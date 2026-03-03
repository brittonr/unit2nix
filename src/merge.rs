use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

use anyhow::{bail, Result};

use crate::unit_graph::{UnitGraph, UnitMode, Unit};
use crate::metadata::{CargoMetadata, CargoLock, MetadataPackage};
use crate::output::{NixBuildPlan, NixCrate, NixDep, NixBinTarget, BUILD_PLAN_VERSION};
use crate::source::{parse_source, infer_source_from_pkg_id};

/// Extract `(crate_name, version)` from a package ID string.
///
/// Handles these formats:
/// - `registry+...#name@version` → `(name, version)`
/// - `path+file:///.../<name>#version` → `(name, version)`
/// - `git+...#name@version` → `(name, version)`
pub(crate) fn parse_pkg_id(pkg_id: &str) -> Result<(String, String)> {
    let (prefix, fragment) = pkg_id
        .rsplit_once('#')
        .ok_or_else(|| anyhow::anyhow!("malformed package ID (no '#' separator): {pkg_id}"))?;

    if let Some((name, version)) = fragment.rsplit_once('@') {
        return Ok((name.to_string(), version.to_string()));
    }

    // path deps: fragment is just the version, name is in the path
    // e.g., path+file:///home/user/proj/crates/foo#0.1.0
    let name = prefix
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("malformed package ID (no crate name): {pkg_id}"))?
        .to_string();

    Ok((name, fragment.to_string()))
}

/// Merge cargo unit-graph, metadata, and lockfile into a Nix build plan.
///
/// This is the core of unit2nix: it combines three cargo outputs into a single
/// JSON structure that the Nix consumer can walk to build each crate.
///
/// When `test_unit_graph` is provided (from `cargo test --unit-graph`), any
/// packages and dependencies that appear in the test graph but not the build
/// graph are emitted as `devDependencies` on the relevant workspace members.
pub fn merge(
    unit_graph: &UnitGraph,
    metadata: &CargoMetadata,
    lock: &CargoLock,
    target: Option<&str>,
    cargo_lock_hash: String,
    test_unit_graph: Option<&UnitGraph>,
) -> Result<NixBuildPlan> {
    // Index metadata packages by their id
    let meta_by_id: BTreeMap<&str, &MetadataPackage> = metadata
        .packages
        .iter()
        .map(|p| (p.id.as_str(), p))
        .collect();

    // Index Cargo.lock checksums by (name, version)
    let checksums: BTreeMap<(&str, &str), &str> = lock
        .package
        .as_ref()
        .map(|pkgs| {
            pkgs.iter()
                .filter_map(|p| {
                    p.checksum
                        .as_deref()
                        .map(|cksum| ((p.name.as_str(), p.version.as_str()), cksum))
                })
                .collect()
        })
        .unwrap_or_default();

    // Map unit index → pkg_id for dependency resolution
    let unit_pkg_ids: Vec<&str> = unit_graph.units.iter().map(|u| u.pkg_id.as_str()).collect();

    // Group units by pkg_id
    let mut pkg_units: BTreeMap<&str, Vec<(usize, &Unit)>> = BTreeMap::new();
    for (idx, unit) in unit_graph.units.iter().enumerate() {
        pkg_units
            .entry(unit.pkg_id.as_str())
            .or_default()
            .push((idx, unit));
    }

    let mut crates = BTreeMap::new();

    for (pkg_id, units) in &pkg_units {
        // Find the primary lib unit (prefer lib over proc-macro)
        let lib_unit = units
            .iter()
            .find(|(_, u)| u.mode == UnitMode::Build && u.target.has_lib())
            .or_else(|| {
                units.iter().find(|(_, u)| u.mode == UnitMode::Build && u.target.has_proc_macro())
            });

        let bin_units: Vec<&(usize, &Unit)> = units
            .iter()
            .filter(|(_, u)| u.mode == UnitMode::Build && u.target.has_bin())
            .collect();

        let build_script_unit = units
            .iter()
            .find(|(_, u)| u.mode == UnitMode::Build && u.target.has_custom_build());

        // Skip packages with no buildable target (run-custom-build only, etc.)
        let primary_unit = lib_unit.or_else(|| bin_units.first().copied());
        let primary_unit = match primary_unit {
            Some(u) => u,
            None => {
                eprintln!("warning: skipping {pkg_id}: no lib/bin/proc-macro target");
                continue;
            }
        };

        let (_, primary) = primary_unit;
        let (crate_name, version) = parse_pkg_id(pkg_id)?;

        let meta_pkg = meta_by_id.get(pkg_id);

        // Features: union across all lib-like units (different feature variants
        // may exist for the same crate; Nix builds one derivation per crate).
        let features = collect_features(units);
        let proc_macro = primary.target.has_proc_macro();
        let dependencies = collect_dependencies(units, unit_graph, &unit_pkg_ids, pkg_id);
        let build_dependencies = collect_build_dependencies(build_script_unit, &unit_pkg_ids);

        // SHA256 from Cargo.lock
        let sha256 = checksums
            .get(&(crate_name.as_str(), version.as_str()))
            .map(|s| s.to_string());

        // Source info: prefer metadata, fall back to inferring from pkg_id.
        // Some crates appear in the unit graph but not in cargo metadata
        // (e.g., transitive deps pulled in by feature-specific resolution).
        let source = if let Some(m) = meta_pkg {
            match parse_source(m.source.as_deref(), &m.manifest_path, &metadata.workspace_root) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("warning: {e:#} for {crate_name}, falling back to pkg_id inference");
                    infer_source_from_pkg_id(pkg_id)
                }
            }
        } else {
            infer_source_from_pkg_id(pkg_id)
        };

        if source.is_none() && !pkg_id.starts_with("path+") {
            eprintln!(
                "warning: could not determine source for {crate_name} ({pkg_id}), treating as local"
            );
        }

        // Crate root directory (from manifest_path, strip Cargo.toml)
        let crate_root = meta_pkg
            .and_then(|m| Path::new(&m.manifest_path).parent())
            .unwrap_or(Path::new(""));

        // Make an absolute src_path relative to the crate root
        let make_relative = |abs_path: &str| -> String {
            Path::new(abs_path)
                .strip_prefix(crate_root)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| abs_path.to_string())
        };

        // Lib info
        let lib_path = lib_unit.map(|(_, lu)| make_relative(&lu.target.src_path))
            .filter(|p| p != "src/lib.rs");

        let lib_name = lib_unit.and_then(|(_, lu)| {
            let n = lu.target.name.replace('-', "_");
            if n == crate_name.replace('-', "_") { None } else { Some(n) }
        });

        let lib_crate_types = lib_unit
            .map(|(_, lu)| lu.target.crate_types.clone())
            .unwrap_or_default();

        // Binary targets (only for workspace members / roots)
        let is_workspace_member = metadata
            .workspace_members
            .iter()
            .any(|wm| wm == pkg_id);
        let crate_bin: Vec<NixBinTarget> = if is_workspace_member {
            bin_units
                .iter()
                .map(|(_, bu)| NixBinTarget {
                    name: bu.target.name.clone(),
                    path: make_relative(&bu.target.src_path),
                })
                .collect()
        } else {
            vec![]
        };

        let links = meta_pkg.and_then(|m| m.links.clone());

        // Build script path (relative to crate root, None if standard build.rs)
        let build = build_script_unit.and_then(|(_, bs_unit)| {
            let rel = make_relative(&bs_unit.target.src_path);
            if rel == "build.rs" { None } else { Some(rel) }
        });

        // Package metadata for CARGO_PKG_* env vars.
        // Sanitize strings: buildRustCrate exports these via bash using
        // `export CARGO_PKG_DESCRIPTION="..."`, so newlines and embedded
        // double-quotes break the export.
        let sanitize = |s: String| {
            s.replace(['\n', '\r'], " ")
             .replace('"', "'")
        };
        let authors = meta_pkg.and_then(|m| m.authors.clone()).unwrap_or_default();
        let description = meta_pkg.and_then(|m| m.description.clone()).map(sanitize);
        let homepage = meta_pkg.and_then(|m| m.homepage.clone());
        let license = meta_pkg.and_then(|m| m.license.clone());
        let repository = meta_pkg.and_then(|m| m.repository.clone());

        crates.insert(
            pkg_id.to_string(),
            NixCrate {
                crate_name,
                version,
                edition: primary.target.edition.clone(),
                sha256,
                source,
                features,
                dependencies,
                build_dependencies,
                dev_dependencies: vec![],  // populated below from test graph
                proc_macro,
                build,
                lib_path,
                lib_name,
                lib_crate_types,
                crate_bin,
                links,
                authors,
                description,
                homepage,
                license,
                repository,
            },
        );
    }

    // Dev dependencies: compare test unit graph against build unit graph to find
    // dev-only packages and dependencies for workspace members.
    if let Some(test_graph) = test_unit_graph {
        compute_dev_dependencies(test_graph, unit_graph, metadata, &meta_by_id, &checksums, &mut crates)?;
    }

    // Roots
    let roots: Vec<String> = unit_graph
        .roots
        .iter()
        .map(|&idx| {
            unit_graph.units.get(idx)
                .unwrap_or_else(|| panic!(
                    "root index {idx} out of range (len {})", unit_graph.units.len(),
                ))
                .pkg_id.clone()
        })
        .collect();

    // Workspace members: map name → package ID for members present in the build plan.
    // Only includes members that actually appear in the resolved dependency graph,
    // not all workspace members (some may be excluded by feature/package selection).
    let workspace_members: BTreeMap<String, String> = metadata
        .workspace_members
        .iter()
        .filter_map(|wm_id| {
            if crates.contains_key(wm_id) {
                let name = crates[wm_id].crate_name.clone();
                Some((name, wm_id.clone()))
            } else {
                None
            }
        })
        .collect();

    validate_references(&crates)?;

    Ok(NixBuildPlan {
        version: BUILD_PLAN_VERSION,
        workspace_root: metadata.workspace_root.clone(),
        roots,
        workspace_members,
        target: target.map(|s| s.to_string()),
        cargo_lock_hash,
        crates,
    })
}

/// Collect deduplicated, sorted features across all lib-like units for a package.
///
/// The same crate can appear multiple times in the unit graph with different
/// feature sets (e.g., `hashbrown`: once with no features for a proc-macro's
/// host dep, once with `"default"` for a target dep). Nix builds one derivation
/// per crate, so it needs the superset.
fn collect_features(units: &[(usize, &Unit)]) -> Vec<String> {
    let mut features = BTreeSet::new();
    for (_, u) in units {
        if u.mode == UnitMode::Build && u.target.has_lib_like() {
            for f in &u.features {
                features.insert(f.clone());
            }
        }
    }
    features.into_iter().collect()
}

/// Collect deduplicated normal dependencies from all buildable units.
///
/// Unions deps across the primary lib/proc-macro unit and all bin units,
/// since different feature variants may pull in different deps.
fn collect_dependencies(
    units: &[(usize, &Unit)],
    unit_graph: &UnitGraph,
    unit_pkg_ids: &[&str],
    pkg_id: &str,
) -> Vec<NixDep> {
    let mut deps = Vec::new();
    let mut seen = HashSet::new();

    let buildable_units = units.iter().filter(|(_, u)| {
        u.mode == UnitMode::Build
            && (u.target.has_lib_like() || u.target.has_bin())
    });

    for (_, u) in buildable_units {
        for dep in &u.dependencies {
            let dep_unit = unit_graph.units.get(dep.index)
                .unwrap_or_else(|| panic!(
                    "dependency index {} out of range (len {}) for {pkg_id}",
                    dep.index, unit_graph.units.len(),
                ));
            let dep_pkg_id = unit_pkg_ids[dep.index];
            // Skip self-references (bin → lib within same package)
            if dep_pkg_id == pkg_id {
                continue;
            }
            if dep_unit.mode == UnitMode::RunCustomBuild {
                continue;
            }
            let key = (dep_pkg_id, dep.extern_crate_name.as_str());
            if seen.insert(key) {
                deps.push(NixDep {
                    package_id: dep_pkg_id.to_string(),
                    extern_crate_name: dep.extern_crate_name.clone(),
                });
            }
        }
    }
    deps
}

/// Collect deduplicated build dependencies from the custom-build (build.rs) unit.
fn collect_build_dependencies(
    build_script_unit: Option<&(usize, &Unit)>,
    unit_pkg_ids: &[&str],
) -> Vec<NixDep> {
    let mut seen = HashSet::new();
    build_script_unit
        .map(|(_, bs_unit)| {
            bs_unit
                .dependencies
                .iter()
                .filter_map(|dep| {
                    let dep_pkg_id = unit_pkg_ids[dep.index];
                    let key = (dep_pkg_id, dep.extern_crate_name.as_str());
                    if seen.insert(key) {
                        Some(NixDep {
                            package_id: dep_pkg_id.to_string(),
                            extern_crate_name: dep.extern_crate_name.clone(),
                        })
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Compute dev-only dependencies by diffing test and build unit graphs.
///
/// For each workspace member, finds dependencies that exist in the test graph
/// but not in the build graph. Also adds any new crates to the build plan that
/// are only needed for testing.
fn compute_dev_dependencies(
    test_graph: &UnitGraph,
    build_graph: &UnitGraph,
    metadata: &CargoMetadata,
    meta_by_id: &BTreeMap<&str, &MetadataPackage>,
    checksums: &BTreeMap<(&str, &str), &str>,
    crates: &mut BTreeMap<String, NixCrate>,
) -> Result<()> {
    // Index build graph: pkg_id → set of dependency pkg_ids (normal deps)
    let build_pkg_ids: Vec<&str> = build_graph.units.iter().map(|u| u.pkg_id.as_str()).collect();
    let mut build_deps_by_pkg: BTreeMap<&str, HashSet<&str>> = BTreeMap::new();
    for unit in &build_graph.units {
        if unit.mode != UnitMode::Build {
            continue;
        }
        if !unit.target.has_lib_like() && !unit.target.has_bin() {
            continue;
        }
        let entry = build_deps_by_pkg.entry(unit.pkg_id.as_str()).or_default();
        for dep in &unit.dependencies {
            let dep_pkg_id = build_pkg_ids[dep.index];
            if dep_pkg_id != unit.pkg_id.as_str() {
                entry.insert(dep_pkg_id);
            }
        }
    }

    // Index test graph — include both Build and Test mode units.
    // Dev dependencies only appear on mode=test units (lib compiled for testing),
    // not on mode=build units.
    let test_pkg_ids: Vec<&str> = test_graph.units.iter().map(|u| u.pkg_id.as_str()).collect();
    let mut test_deps_by_pkg: BTreeMap<&str, Vec<NixDep>> = BTreeMap::new();
    for unit in &test_graph.units {
        // Include Build (normal) and Test (lib-as-test) modes.
        // Dev dependencies appear on mode=test units in `cargo test --unit-graph`.
        match unit.mode {
            UnitMode::Build | UnitMode::Test => {}
            _ => continue,
        }
        if !unit.target.has_lib_like() && !unit.target.has_bin() {
            continue;
        }
        let entry = test_deps_by_pkg.entry(unit.pkg_id.as_str()).or_default();
        for dep in &unit.dependencies {
            let dep_unit = &test_graph.units[dep.index];
            let dep_pkg_id = test_pkg_ids[dep.index];
            if dep_pkg_id == unit.pkg_id.as_str() {
                continue;
            }
            if dep_unit.mode == UnitMode::RunCustomBuild {
                continue;
            }
            entry.push(NixDep {
                package_id: dep_pkg_id.to_string(),
                extern_crate_name: dep.extern_crate_name.clone(),
            });
        }
    }

    // Identify workspace member pkg_ids
    let ws_member_ids: HashSet<&str> = metadata
        .workspace_members
        .iter()
        .map(|s| s.as_str())
        .collect();

    // First pass: collect all pkg_ids that are dev-only (in test graph but not build plan)
    let mut dev_only_pkg_ids: BTreeSet<String> = BTreeSet::new();
    for pkg_id in test_pkg_ids.iter() {
        if !crates.contains_key(*pkg_id) {
            dev_only_pkg_ids.insert(pkg_id.to_string());
        }
    }

    // Add dev-only crates to the build plan (they're needed as dependencies)
    let mut test_pkg_units: BTreeMap<&str, Vec<(usize, &crate::unit_graph::Unit)>> = BTreeMap::new();
    for (idx, unit) in test_graph.units.iter().enumerate() {
        test_pkg_units
            .entry(unit.pkg_id.as_str())
            .or_default()
            .push((idx, unit));
    }

    for dev_pkg_id in &dev_only_pkg_ids {
        let (crate_name, version) = parse_pkg_id(dev_pkg_id)?;
        let meta_pkg = meta_by_id.get(dev_pkg_id.as_str());

        let sha256 = checksums
            .get(&(crate_name.as_str(), version.as_str()))
            .map(|s| s.to_string());

        let source = if let Some(m) = meta_pkg {
            match parse_source(m.source.as_deref(), &m.manifest_path, &metadata.workspace_root) {
                Ok(s) => s,
                Err(_) => infer_source_from_pkg_id(dev_pkg_id),
            }
        } else {
            infer_source_from_pkg_id(dev_pkg_id)
        };

        // Collect features and deps from test graph units for this package
        let units = test_pkg_units.get(dev_pkg_id.as_str()).cloned().unwrap_or_default();
        let features = collect_features(&units);
        let dependencies = collect_dependencies(&units, test_graph, &test_pkg_ids, dev_pkg_id);

        let build_script_unit = units
            .iter()
            .find(|(_, u)| u.mode == UnitMode::Build && u.target.has_custom_build());
        let build_dependencies = collect_build_dependencies(build_script_unit, &test_pkg_ids);

        let lib_unit = units
            .iter()
            .find(|(_, u)| u.mode == UnitMode::Build && u.target.has_lib())
            .or_else(|| units.iter().find(|(_, u)| u.mode == UnitMode::Build && u.target.has_proc_macro()));

        let primary_unit = lib_unit.or_else(|| {
            units.iter().find(|(_, u)| u.mode == UnitMode::Build && u.target.has_bin())
        });
        let primary = match primary_unit {
            Some((_, u)) => u,
            None => continue, // skip if no buildable target
        };

        let crate_root = meta_pkg
            .and_then(|m| std::path::Path::new(&m.manifest_path).parent())
            .unwrap_or(std::path::Path::new(""));

        let make_relative = |abs_path: &str| -> String {
            std::path::Path::new(abs_path)
                .strip_prefix(crate_root)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| abs_path.to_string())
        };

        let lib_path = lib_unit.map(|(_, lu)| make_relative(&lu.target.src_path))
            .filter(|p| p != "src/lib.rs");
        let lib_name = lib_unit.and_then(|(_, lu)| {
            let n = lu.target.name.replace('-', "_");
            if n == crate_name.replace('-', "_") { None } else { Some(n) }
        });
        let lib_crate_types = lib_unit
            .map(|(_, lu)| lu.target.crate_types.clone())
            .unwrap_or_default();
        let proc_macro = primary.target.has_proc_macro();
        let links = meta_pkg.and_then(|m| m.links.clone());
        let build = units
            .iter()
            .find(|(_, u)| u.mode == UnitMode::Build && u.target.has_custom_build())
            .and_then(|(_, bs_unit)| {
                let rel = make_relative(&bs_unit.target.src_path);
                if rel == "build.rs" { None } else { Some(rel) }
            });

        let sanitize = |s: String| s.replace(['\n', '\r'], " ").replace('"', "'");
        let authors = meta_pkg.and_then(|m| m.authors.clone()).unwrap_or_default();
        let description = meta_pkg.and_then(|m| m.description.clone()).map(sanitize);
        let homepage = meta_pkg.and_then(|m| m.homepage.clone());
        let license = meta_pkg.and_then(|m| m.license.clone());
        let repository = meta_pkg.and_then(|m| m.repository.clone());

        crates.insert(
            dev_pkg_id.to_string(),
            NixCrate {
                crate_name,
                version,
                edition: primary.target.edition.clone(),
                sha256,
                source,
                features,
                dependencies,
                build_dependencies,
                dev_dependencies: vec![],
                proc_macro,
                build,
                lib_path,
                lib_name,
                lib_crate_types,
                crate_bin: vec![],
                links,
                authors,
                description,
                homepage,
                license,
                repository,
            },
        );
    }

    // Second pass: compute dev-only deps for each workspace member
    for ws_id in &ws_member_ids {
        let build_dep_ids = build_deps_by_pkg.get(ws_id).cloned().unwrap_or_default();
        let test_deps = test_deps_by_pkg.get(ws_id).cloned().unwrap_or_default();

        // Dev deps = deps in test graph but not in build graph for this member
        let mut seen = HashSet::new();
        let dev_deps: Vec<NixDep> = test_deps
            .into_iter()
            .filter(|dep| {
                !build_dep_ids.contains(dep.package_id.as_str())
                    && seen.insert((dep.package_id.clone(), dep.extern_crate_name.clone()))
            })
            .collect();

        if !dev_deps.is_empty() {
            if let Some(crate_info) = crates.get_mut(*ws_id) {
                crate_info.dev_dependencies = dev_deps;
            }
        }
    }

    Ok(())
}

/// Validate that every dependency reference resolves to a crate in the plan.
fn validate_references(crates: &BTreeMap<String, NixCrate>) -> Result<()> {
    let mut missing_refs: Vec<(String, String)> = Vec::new();
    for (pkg_id, crate_info) in crates {
        let all_deps = crate_info.dependencies.iter()
            .chain(&crate_info.build_dependencies)
            .chain(&crate_info.dev_dependencies);
        for dep in all_deps {
            if !crates.contains_key(&dep.package_id) {
                missing_refs.push((pkg_id.clone(), dep.package_id.clone()));
            }
        }
    }
    if !missing_refs.is_empty() {
        eprintln!("ERROR: {} dangling dependency references:", missing_refs.len());
        for (from, to) in &missing_refs {
            let from_name = crates.get(from).map(|c| c.crate_name.as_str()).unwrap_or("?");
            eprintln!("  {from_name} ({from}) -> {to}");
        }
        bail!(
            "{} dependencies reference crates not in the build plan \
             (likely a missing crate kind — see unit2nix bug tracker)",
            missing_refs.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_pkg_id;

    #[test]
    fn parse_registry_pkg_id() {
        let (name, version) =
            parse_pkg_id("registry+https://github.com/rust-lang/crates.io-index#serde@1.0.200")
                .unwrap();
        assert_eq!(name, "serde");
        assert_eq!(version, "1.0.200");
    }

    #[test]
    fn parse_path_pkg_id() {
        let (name, version) =
            parse_pkg_id("path+file:///home/user/project/crates/aspen-core#0.1.0").unwrap();
        assert_eq!(name, "aspen-core");
        assert_eq!(version, "0.1.0");
    }

    #[test]
    fn parse_git_pkg_id() {
        let (name, version) =
            parse_pkg_id("git+https://github.com/example/repo.git?rev=abc123#my-crate@0.5.0")
                .unwrap();
        assert_eq!(name, "my-crate");
        assert_eq!(version, "0.5.0");
    }

    #[test]
    fn parse_pkg_id_malformed_no_hash() {
        let result = parse_pkg_id("garbage-with-no-hash");
        assert!(result.is_err(), "should error on malformed pkg_id");
    }
}
