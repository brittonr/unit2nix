use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;

use anyhow::{bail, Result};

use crate::metadata::{CargoLock, CargoMetadata, MetadataPackage};
use crate::output::{NixBuildPlan, NixBinTarget, NixCrate, NixDep, NixSource, BUILD_PLAN_VERSION};
use crate::source::{infer_source_from_pkg_id, parse_source};
use crate::unit_graph::{Unit, UnitGraph, UnitMode};

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

// ---------------------------------------------------------------------------
// Shared helpers for NixCrate construction
// ---------------------------------------------------------------------------

/// Make an absolute path relative to a crate root directory.
fn make_relative(abs_path: &str, crate_root: &Path) -> String {
    Path::new(abs_path)
        .strip_prefix(crate_root)
        .map_or_else(
            |_| abs_path.to_string(),
            |p| p.to_string_lossy().into_owned(),
        )
}

/// Sanitize a string for safe use in bash `export` statements.
///
/// `buildRustCrate` exports `CARGO_PKG_*` env vars via bash using
/// `export CARGO_PKG_DESCRIPTION="..."`, so newlines and embedded
/// double-quotes break the export.
fn sanitize_metadata(s: &str) -> String {
    s.replace(['\n', '\r'], " ").replace('"', "'")
}

/// Resolve source info for a crate, preferring metadata over `pkg_id` inference.
fn resolve_source(
    pkg_id: &str,
    crate_name: &str,
    meta_pkg: Option<&MetadataPackage>,
    workspace_root: &str,
) -> Option<NixSource> {
    let source = if let Some(m) = meta_pkg {
        match parse_source(m.source.as_deref(), &m.manifest_path, workspace_root) {
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

    source
}

/// Build a `NixCrate` from a set of unit graph units for a package.
///
/// Returns `Ok(Some(crate))` on success, `Ok(None)` if the package has no
/// buildable target (should be skipped), or `Err` on parse failures.
#[allow(clippy::too_many_arguments)]
fn build_nix_crate(
    pkg_id: &str,
    units: &[(usize, &Unit)],
    unit_graph: &UnitGraph,
    unit_pkg_ids: &[&str],
    meta_pkg: Option<&MetadataPackage>,
    checksums: &BTreeMap<(&str, &str), &str>,
    workspace_root: &str,
    include_bins: bool,
) -> Result<Option<NixCrate>> {
    // Find the primary lib unit (prefer lib over proc-macro)
    let lib_unit = units
        .iter()
        .find(|(_, u)| u.mode == UnitMode::Build && u.target.has_lib())
        .or_else(|| {
            units
                .iter()
                .find(|(_, u)| u.mode == UnitMode::Build && u.target.has_proc_macro())
        });

    let bin_units: Vec<&(usize, &Unit)> = units
        .iter()
        .filter(|(_, u)| u.mode == UnitMode::Build && u.target.has_bin())
        .collect();

    let build_script_unit = units
        .iter()
        .find(|(_, u)| u.mode == UnitMode::Build && u.target.has_custom_build());

    // Skip packages with no buildable target (run-custom-build only, etc.)
    let Some((_, primary)) = lib_unit.or_else(|| bin_units.first().copied()) else {
        eprintln!("warning: skipping {pkg_id}: no lib/bin/proc-macro target");
        return Ok(None);
    };

    let (crate_name, version) = parse_pkg_id(pkg_id)?;

    let features = collect_features(units);
    let proc_macro = primary.target.has_proc_macro();
    let dependencies = collect_dependencies(units, unit_graph, unit_pkg_ids, pkg_id);
    let build_dependencies = collect_build_dependencies(build_script_unit, unit_pkg_ids);

    let sha256 = checksums
        .get(&(crate_name.as_str(), version.as_str()))
        .map(std::string::ToString::to_string);

    let source = resolve_source(pkg_id, &crate_name, meta_pkg, workspace_root);

    // Crate root directory (from manifest_path, strip Cargo.toml)
    let crate_root = meta_pkg
        .and_then(|m| Path::new(&m.manifest_path).parent())
        .unwrap_or(Path::new(""));

    let lib_path = lib_unit
        .map(|(_, lu)| make_relative(&lu.target.src_path, crate_root))
        .filter(|p| p != "src/lib.rs");

    let lib_name = lib_unit.and_then(|(_, lu)| {
        let n = lu.target.name.replace('-', "_");
        if n == crate_name.replace('-', "_") {
            None
        } else {
            Some(n)
        }
    });

    let lib_crate_types = lib_unit
        .map(|(_, lu)| lu.target.crate_types.clone())
        .unwrap_or_default();

    let crate_bin: Vec<NixBinTarget> = if include_bins {
        bin_units
            .iter()
            .map(|(_, bu)| NixBinTarget {
                name: bu.target.name.clone(),
                path: make_relative(&bu.target.src_path, crate_root),
            })
            .collect()
    } else {
        vec![]
    };

    let links = meta_pkg.and_then(|m| m.links.clone());

    let build = build_script_unit.and_then(|(_, bs_unit)| {
        let rel = make_relative(&bs_unit.target.src_path, crate_root);
        if rel == "build.rs" {
            None
        } else {
            Some(rel)
        }
    });

    let authors = meta_pkg
        .and_then(|m| m.authors.clone())
        .unwrap_or_default();
    let description = meta_pkg
        .and_then(|m| m.description.clone())
        .map(|s| sanitize_metadata(&s));
    let homepage = meta_pkg.and_then(|m| m.homepage.clone());
    let license = meta_pkg.and_then(|m| m.license.clone());
    let repository = meta_pkg.and_then(|m| m.repository.clone());

    Ok(Some(NixCrate {
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
        crate_bin,
        links,
        authors,
        description,
        homepage,
        license,
        repository,
    }))
}

// ---------------------------------------------------------------------------
// Core merge logic
// ---------------------------------------------------------------------------

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
    members_filter: Option<&[String]>,
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
        let meta_pkg = meta_by_id.get(pkg_id).copied();
        let is_workspace_member = metadata.workspace_members.iter().any(|wm| wm == pkg_id);

        let Some(nix_crate) = build_nix_crate(
            pkg_id,
            units,
            unit_graph,
            &unit_pkg_ids,
            meta_pkg,
            &checksums,
            &metadata.workspace_root,
            is_workspace_member,
        )?
        else {
            continue;
        };

        crates.insert(pkg_id.to_string(), nix_crate);
    }

    // Dev dependencies: compare test unit graph against build unit graph to find
    // dev-only packages and dependencies for workspace members.
    if let Some(test_graph) = test_unit_graph {
        compute_dev_dependencies(
            test_graph,
            unit_graph,
            metadata,
            &meta_by_id,
            &checksums,
            &mut crates,
        )?;
    }

    // Roots
    let roots: Vec<String> = unit_graph
        .roots
        .iter()
        .map(|&idx| {
            unit_graph
                .units
                .get(idx)
                .unwrap_or_else(|| {
                    panic!(
                        "root index {idx} out of range (len {})",
                        unit_graph.units.len(),
                    )
                })
                .pkg_id
                .clone()
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

    // Apply workspace member filtering (--members flag)
    let (filtered_roots, filtered_workspace_members) = if let Some(filter) = members_filter {
        // Validate all requested names exist
        let valid_names: Vec<&str> = workspace_members.keys().map(String::as_str).collect();
        for name in filter {
            if !workspace_members.contains_key(name) {
                bail!(
                    "unknown workspace member '{name}'. Valid members: {}",
                    valid_names.join(", ")
                );
            }
        }

        // Filter workspace_members to only requested names
        let filtered_wm: BTreeMap<String, String> = workspace_members
            .into_iter()
            .filter(|(name, _)| filter.contains(name))
            .collect();

        // Filter roots to only package IDs of selected members
        let member_pkg_ids: std::collections::HashSet<&str> =
            filtered_wm.values().map(String::as_str).collect();
        let filtered_roots: Vec<String> = roots
            .into_iter()
            .filter(|r| member_pkg_ids.contains(r.as_str()))
            .collect();

        (filtered_roots, filtered_wm)
    } else {
        (roots, workspace_members)
    };

    Ok(NixBuildPlan {
        version: BUILD_PLAN_VERSION,
        workspace_root: metadata.workspace_root.clone(),
        roots: filtered_roots,
        workspace_members: filtered_workspace_members,
        target: target.map(str::to_owned),
        cargo_lock_hash,
        crates,
    })
}

// ---------------------------------------------------------------------------
// Dependency / feature collection helpers
// ---------------------------------------------------------------------------

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

    let buildable_units = units
        .iter()
        .filter(|(_, u)| u.mode == UnitMode::Build && (u.target.has_lib_like() || u.target.has_bin()));

    for (_, u) in buildable_units {
        for dep in &u.dependencies {
            let dep_unit = unit_graph.units.get(dep.index).unwrap_or_else(|| {
                panic!(
                    "dependency index {} out of range (len {}) for {pkg_id}",
                    dep.index,
                    unit_graph.units.len(),
                )
            });
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

// ---------------------------------------------------------------------------
// Dev dependency computation
// ---------------------------------------------------------------------------

/// Index a unit graph into a map of `pkg_id` → set of dependency `pkg_ids`.
///
/// Used to build the "build graph" index for diffing against the test graph.
fn index_build_deps(graph: &UnitGraph) -> BTreeMap<&str, HashSet<&str>> {
    let pkg_ids: Vec<&str> = graph.units.iter().map(|u| u.pkg_id.as_str()).collect();
    let mut deps_by_pkg: BTreeMap<&str, HashSet<&str>> = BTreeMap::new();
    for unit in &graph.units {
        if unit.mode != UnitMode::Build {
            continue;
        }
        if !unit.target.has_lib_like() && !unit.target.has_bin() {
            continue;
        }
        let entry = deps_by_pkg.entry(unit.pkg_id.as_str()).or_default();
        for dep in &unit.dependencies {
            let dep_pkg_id = pkg_ids[dep.index];
            if dep_pkg_id != unit.pkg_id.as_str() {
                entry.insert(dep_pkg_id);
            }
        }
    }
    deps_by_pkg
}

/// Index a test unit graph into a map of `pkg_id` → list of `NixDep`.
///
/// Includes both Build and Test mode units (dev dependencies appear on
/// mode=test units in `cargo test --unit-graph`).
fn index_test_deps(graph: &UnitGraph) -> (Vec<&str>, BTreeMap<&str, Vec<NixDep>>) {
    let pkg_ids: Vec<&str> = graph.units.iter().map(|u| u.pkg_id.as_str()).collect();
    let mut deps_by_pkg: BTreeMap<&str, Vec<NixDep>> = BTreeMap::new();
    for unit in &graph.units {
        match unit.mode {
            UnitMode::Build | UnitMode::Test => {}
            _ => continue,
        }
        if !unit.target.has_lib_like() && !unit.target.has_bin() {
            continue;
        }
        let entry = deps_by_pkg.entry(unit.pkg_id.as_str()).or_default();
        for dep in &unit.dependencies {
            let dep_unit = &graph.units[dep.index];
            let dep_pkg_id = pkg_ids[dep.index];
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
    (pkg_ids, deps_by_pkg)
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
    let build_deps_by_pkg = index_build_deps(build_graph);
    let (test_pkg_ids, test_deps_by_pkg) = index_test_deps(test_graph);

    // Identify workspace member pkg_ids
    let ws_member_ids: HashSet<&str> = metadata
        .workspace_members
        .iter()
        .map(String::as_str)
        .collect();

    // First pass: collect all pkg_ids that are dev-only (in test graph but not build plan)
    let mut dev_only_pkg_ids: BTreeSet<String> = BTreeSet::new();
    for pkg_id in &test_pkg_ids {
        if !crates.contains_key(*pkg_id) {
            dev_only_pkg_ids.insert((*pkg_id).to_string());
        }
    }

    // Group test graph units by pkg_id for build_nix_crate
    let mut test_pkg_units: BTreeMap<&str, Vec<(usize, &Unit)>> = BTreeMap::new();
    for (idx, unit) in test_graph.units.iter().enumerate() {
        test_pkg_units
            .entry(unit.pkg_id.as_str())
            .or_default()
            .push((idx, unit));
    }

    // Add dev-only crates to the build plan (they're needed as dependencies)
    for dev_pkg_id in &dev_only_pkg_ids {
        let meta_pkg = meta_by_id.get(dev_pkg_id.as_str()).copied();
        let units = test_pkg_units
            .get(dev_pkg_id.as_str())
            .cloned()
            .unwrap_or_default();

        let Some(nix_crate) = build_nix_crate(
            dev_pkg_id,
            &units,
            test_graph,
            &test_pkg_ids,
            meta_pkg,
            checksums,
            &metadata.workspace_root,
            false, // dev-only crates never include bin targets
        )?
        else {
            continue;
        };

        crates.insert(dev_pkg_id.clone(), nix_crate);
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

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate that every dependency reference resolves to a crate in the plan.
fn validate_references(crates: &BTreeMap<String, NixCrate>) -> Result<()> {
    let mut missing_refs: Vec<(String, String)> = Vec::new();
    for (pkg_id, crate_info) in crates {
        let all_deps = crate_info
            .dependencies
            .iter()
            .chain(&crate_info.build_dependencies)
            .chain(&crate_info.dev_dependencies);
        for dep in all_deps {
            if !crates.contains_key(&dep.package_id) {
                missing_refs.push((pkg_id.clone(), dep.package_id.clone()));
            }
        }
    }
    if !missing_refs.is_empty() {
        eprintln!("error: {} dangling dependency references:", missing_refs.len());
        for (from, to) in &missing_refs {
            let from_name = crates
                .get(from)
                .map_or("?", |c| c.crate_name.as_str());
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
    use super::*;
    use crate::metadata::{CargoLock, CargoMetadata, LockPackage, MetadataPackage};
    use crate::output::{NixCrate, NixDep};
    use crate::unit_graph::{CrateKind, Unit, UnitDep, UnitGraph, UnitMode, UnitTarget};
    use std::collections::BTreeMap;
    use std::path::Path;

    // ---------------------------------------------------------------------------
    // Test fixtures / helpers
    // ---------------------------------------------------------------------------

    fn make_unit(
        pkg_id: &str,
        kind: Vec<CrateKind>,
        mode: UnitMode,
        features: Vec<&str>,
        deps: Vec<(usize, &str)>,
    ) -> Unit {
        Unit {
            pkg_id: pkg_id.to_string(),
            target: UnitTarget {
                kind,
                crate_types: vec!["lib".to_string()],
                name: "test".to_string(),
                src_path: "/path/to/src/lib.rs".to_string(),
                edition: "2021".to_string(),
            },
            mode,
            features: features.iter().map(|s| s.to_string()).collect(),
            dependencies: deps
                .iter()
                .map(|(idx, name)| UnitDep {
                    index: *idx,
                    extern_crate_name: name.to_string(),
                })
                .collect(),
        }
    }

    fn make_meta_pkg(id: &str, source: Option<&str>, manifest_path: &str) -> MetadataPackage {
        MetadataPackage {
            id: id.to_string(),
            source: source.map(str::to_string),
            manifest_path: manifest_path.to_string(),
            links: None,
            authors: Some(vec!["Test Author <test@example.com>".to_string()]),
            description: Some("A test package".to_string()),
            homepage: None,
            license: Some("MIT".to_string()),
            repository: None,
        }
    }

    fn make_lock_pkg(name: &str, version: &str, checksum: Option<&str>) -> LockPackage {
        LockPackage {
            name: name.to_string(),
            version: version.to_string(),
            checksum: checksum.map(str::to_string),
        }
    }

    // ---------------------------------------------------------------------------
    // parse_pkg_id tests (existing)
    // ---------------------------------------------------------------------------

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

    // ---------------------------------------------------------------------------
    // make_relative tests
    // ---------------------------------------------------------------------------

    #[test]
    fn make_relative_strips_prefix() {
        let crate_root = Path::new("/home/user/project/crates/foo");
        let abs_path = "/home/user/project/crates/foo/src/lib.rs";
        let result = make_relative(abs_path, crate_root);
        assert_eq!(result, "src/lib.rs");
    }

    #[test]
    fn make_relative_returns_original_on_mismatch() {
        let crate_root = Path::new("/home/user/project/crates/foo");
        let abs_path = "/other/path/src/lib.rs";
        let result = make_relative(abs_path, crate_root);
        assert_eq!(result, abs_path);
    }

    // ---------------------------------------------------------------------------
    // sanitize_metadata tests
    // ---------------------------------------------------------------------------

    #[test]
    fn sanitize_metadata_replaces_newlines() {
        let input = "Line one\nLine two\r\nLine three";
        let result = sanitize_metadata(input);
        // Note: \r\n becomes two spaces because both \n and \r are replaced
        assert_eq!(result, "Line one Line two  Line three");
        assert!(!result.contains('\n'));
        assert!(!result.contains('\r'));
    }

    #[test]
    fn sanitize_metadata_replaces_quotes() {
        let input = r#"Some "quoted" text"#;
        let result = sanitize_metadata(input);
        assert_eq!(result, "Some 'quoted' text");
        assert!(!result.contains('"'));
    }

    // ---------------------------------------------------------------------------
    // collect_features tests
    // ---------------------------------------------------------------------------

    #[test]
    fn collect_features_deduplicates_and_sorts() {
        let unit0 = make_unit(
            "pkg#0.1.0",
            vec![CrateKind::Lib],
            UnitMode::Build,
            vec!["default", "feature_a"],
            vec![],
        );
        let unit1 = make_unit(
            "pkg#0.1.0",
            vec![CrateKind::Lib],
            UnitMode::Build,
            vec!["feature_b", "default"],
            vec![],
        );
        let units = vec![(0, &unit0), (1, &unit1)];
        let features = collect_features(&units);
        assert_eq!(features, vec!["default", "feature_a", "feature_b"]);
    }

    #[test]
    fn collect_features_only_from_build_mode() {
        let unit0 = make_unit(
            "pkg#0.1.0",
            vec![CrateKind::Lib],
            UnitMode::Build,
            vec!["feature_a"],
            vec![],
        );
        let unit1 = make_unit(
            "pkg#0.1.0",
            vec![CrateKind::Lib],
            UnitMode::Test,
            vec!["test_feature"],
            vec![],
        );
        let unit2 = make_unit(
            "pkg#0.1.0",
            vec![CrateKind::Lib],
            UnitMode::RunCustomBuild,
            vec!["build_feature"],
            vec![],
        );
        let units = vec![(0, &unit0), (1, &unit1), (2, &unit2)];
        let features = collect_features(&units);
        assert_eq!(features, vec!["feature_a"]);
    }

    #[test]
    fn collect_features_only_lib_like_targets() {
        let unit0 = make_unit(
            "pkg#0.1.0",
            vec![CrateKind::Lib],
            UnitMode::Build,
            vec!["lib_feature"],
            vec![],
        );
        let unit1 = make_unit(
            "pkg#0.1.0",
            vec![CrateKind::CustomBuild],
            UnitMode::Build,
            vec!["build_feature"],
            vec![],
        );
        let units = vec![(0, &unit0), (1, &unit1)];
        let features = collect_features(&units);
        assert_eq!(features, vec!["lib_feature"]);
    }

    // ---------------------------------------------------------------------------
    // collect_dependencies tests
    // ---------------------------------------------------------------------------

    #[test]
    fn collect_dependencies_filters_self_refs() {
        let pkg_id = "pkg#0.1.0";
        let unit_graph = UnitGraph {
            units: vec![
                make_unit(pkg_id, vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit(pkg_id, vec![CrateKind::Bin], UnitMode::Build, vec![], vec![(0, "pkg")]),
            ],
            roots: vec![0],
        };
        let unit_pkg_ids: Vec<&str> = unit_graph.units.iter().map(|u| u.pkg_id.as_str()).collect();
        let units = vec![(1, &unit_graph.units[1])];

        let deps = collect_dependencies(&units, &unit_graph, &unit_pkg_ids, pkg_id);
        assert!(deps.is_empty(), "bin→lib self-refs should be filtered");
    }

    #[test]
    fn collect_dependencies_skips_run_custom_build() {
        let unit_graph = UnitGraph {
            units: vec![
                make_unit("pkg#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit(
                    "dep#0.2.0",
                    vec![CrateKind::CustomBuild],
                    UnitMode::RunCustomBuild,
                    vec![],
                    vec![],
                ),
                make_unit(
                    "pkg#0.1.0",
                    vec![CrateKind::Lib],
                    UnitMode::Build,
                    vec![],
                    vec![(1, "dep")],
                ),
            ],
            roots: vec![0],
        };
        let unit_pkg_ids: Vec<&str> = unit_graph.units.iter().map(|u| u.pkg_id.as_str()).collect();
        let units = vec![(2, &unit_graph.units[2])];

        let deps = collect_dependencies(&units, &unit_graph, &unit_pkg_ids, "pkg#0.1.0");
        assert!(deps.is_empty(), "RunCustomBuild deps should be skipped");
    }

    #[test]
    fn collect_dependencies_deduplicates() {
        let unit_graph = UnitGraph {
            units: vec![
                make_unit("pkg#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(1, "dep")]),
                make_unit("dep#0.2.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit("pkg#0.1.0", vec![CrateKind::Bin], UnitMode::Build, vec![], vec![(1, "dep")]),
            ],
            roots: vec![0],
        };
        let unit_pkg_ids: Vec<&str> = unit_graph.units.iter().map(|u| u.pkg_id.as_str()).collect();
        let units = vec![(0, &unit_graph.units[0]), (2, &unit_graph.units[2])];

        let deps = collect_dependencies(&units, &unit_graph, &unit_pkg_ids, "pkg#0.1.0");
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].package_id, "dep#0.2.0");
        assert_eq!(deps[0].extern_crate_name, "dep");
    }

    // ---------------------------------------------------------------------------
    // collect_build_dependencies tests
    // ---------------------------------------------------------------------------

    #[test]
    fn collect_build_dependencies_from_build_script() {
        let build_script = make_unit(
            "pkg#0.1.0",
            vec![CrateKind::CustomBuild],
            UnitMode::Build,
            vec![],
            vec![(0, "build_dep"), (1, "another_dep")],
        );
        let unit_pkg_ids = vec!["build_dep#0.1.0", "another_dep#0.2.0"];

        let deps = collect_build_dependencies(Some(&(0, &build_script)), &unit_pkg_ids);
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].package_id, "build_dep#0.1.0");
        assert_eq!(deps[0].extern_crate_name, "build_dep");
        assert_eq!(deps[1].package_id, "another_dep#0.2.0");
        assert_eq!(deps[1].extern_crate_name, "another_dep");
    }

    #[test]
    fn collect_build_dependencies_returns_empty_when_none() {
        let deps = collect_build_dependencies(None, &[]);
        assert!(deps.is_empty());
    }

    // ---------------------------------------------------------------------------
    // validate_references tests
    // ---------------------------------------------------------------------------

    #[test]
    fn validate_references_passes_when_all_valid() {
        let mut crates = BTreeMap::new();
        crates.insert(
            "pkg_a#0.1.0".to_string(),
            NixCrate {
                crate_name: "pkg_a".to_string(),
                version: "0.1.0".to_string(),
                edition: "2021".to_string(),
                sha256: None,
                source: None,
                features: vec![],
                dependencies: vec![NixDep {
                    package_id: "pkg_b#0.2.0".to_string(),
                    extern_crate_name: "pkg_b".to_string(),
                }],
                build_dependencies: vec![],
                dev_dependencies: vec![],
                proc_macro: false,
                build: None,
                lib_path: None,
                lib_name: None,
                lib_crate_types: vec![],
                crate_bin: vec![],
                links: None,
                authors: vec![],
                description: None,
                homepage: None,
                license: None,
                repository: None,
            },
        );
        crates.insert(
            "pkg_b#0.2.0".to_string(),
            NixCrate {
                crate_name: "pkg_b".to_string(),
                version: "0.2.0".to_string(),
                edition: "2021".to_string(),
                sha256: None,
                source: None,
                features: vec![],
                dependencies: vec![],
                build_dependencies: vec![],
                dev_dependencies: vec![],
                proc_macro: false,
                build: None,
                lib_path: None,
                lib_name: None,
                lib_crate_types: vec![],
                crate_bin: vec![],
                links: None,
                authors: vec![],
                description: None,
                homepage: None,
                license: None,
                repository: None,
            },
        );

        let result = validate_references(&crates);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_references_fails_with_dangling_refs() {
        let mut crates = BTreeMap::new();
        crates.insert(
            "pkg_a#0.1.0".to_string(),
            NixCrate {
                crate_name: "pkg_a".to_string(),
                version: "0.1.0".to_string(),
                edition: "2021".to_string(),
                sha256: None,
                source: None,
                features: vec![],
                dependencies: vec![NixDep {
                    package_id: "missing#0.9.0".to_string(),
                    extern_crate_name: "missing".to_string(),
                }],
                build_dependencies: vec![],
                dev_dependencies: vec![],
                proc_macro: false,
                build: None,
                lib_path: None,
                lib_name: None,
                lib_crate_types: vec![],
                crate_bin: vec![],
                links: None,
                authors: vec![],
                description: None,
                homepage: None,
                license: None,
                repository: None,
            },
        );

        let result = validate_references(&crates);
        assert!(result.is_err());
    }

    // ---------------------------------------------------------------------------
    // build_nix_crate tests
    // ---------------------------------------------------------------------------

    #[test]
    fn build_nix_crate_lib_crate() {
        let pkg_id = "registry+https://github.com/rust-lang/crates.io-index#serde@1.0.0";
        let unit_graph = UnitGraph {
            units: vec![make_unit(
                pkg_id,
                vec![CrateKind::Lib],
                UnitMode::Build,
                vec!["default", "std"],
                vec![],
            )],
            roots: vec![0],
        };
        let unit_pkg_ids: Vec<&str> = unit_graph.units.iter().map(|u| u.pkg_id.as_str()).collect();
        let units = vec![(0, &unit_graph.units[0])];

        let meta_pkg = make_meta_pkg(
            pkg_id,
            Some("registry+https://github.com/rust-lang/crates.io-index"),
            "/path/to/Cargo.toml",
        );
        let mut checksums = BTreeMap::new();
        checksums.insert(("serde", "1.0.0"), "abc123");

        let result = build_nix_crate(
            pkg_id,
            &units,
            &unit_graph,
            &unit_pkg_ids,
            Some(&meta_pkg),
            &checksums,
            "/workspace",
            false,
        )
        .unwrap();

        assert!(result.is_some());
        let crate_info = result.unwrap();
        assert_eq!(crate_info.crate_name, "serde");
        assert_eq!(crate_info.version, "1.0.0");
        assert_eq!(crate_info.features, vec!["default", "std"]);
        assert_eq!(crate_info.sha256, Some("abc123".to_string()));
        assert!(!crate_info.proc_macro);
    }

    #[test]
    fn build_nix_crate_bin_only() {
        let pkg_id = "path+file:///home/user/proj#mybin@0.1.0";
        let mut unit = make_unit(pkg_id, vec![CrateKind::Bin], UnitMode::Build, vec![], vec![]);
        unit.target.src_path = "/home/user/proj/src/main.rs".to_string();
        unit.target.name = "mybin".to_string();

        let unit_graph = UnitGraph {
            units: vec![unit],
            roots: vec![0],
        };
        let unit_pkg_ids: Vec<&str> = unit_graph.units.iter().map(|u| u.pkg_id.as_str()).collect();
        let units = vec![(0, &unit_graph.units[0])];

        let meta_pkg = make_meta_pkg(pkg_id, None, "/home/user/proj/Cargo.toml");

        let result = build_nix_crate(
            pkg_id,
            &units,
            &unit_graph,
            &unit_pkg_ids,
            Some(&meta_pkg),
            &BTreeMap::new(),
            "/workspace",
            true, // include_bins = true
        )
        .unwrap();

        assert!(result.is_some());
        let crate_info = result.unwrap();
        assert_eq!(crate_info.crate_name, "mybin");
        assert_eq!(crate_info.crate_bin.len(), 1);
        assert_eq!(crate_info.crate_bin[0].name, "mybin");
    }

    #[test]
    fn build_nix_crate_skips_no_buildable_target() {
        let pkg_id = "pkg#0.1.0";
        let unit_graph = UnitGraph {
            units: vec![make_unit(
                pkg_id,
                vec![CrateKind::CustomBuild],
                UnitMode::RunCustomBuild,
                vec![],
                vec![],
            )],
            roots: vec![0],
        };
        let unit_pkg_ids: Vec<&str> = unit_graph.units.iter().map(|u| u.pkg_id.as_str()).collect();
        let units = vec![(0, &unit_graph.units[0])];

        let result = build_nix_crate(
            pkg_id,
            &units,
            &unit_graph,
            &unit_pkg_ids,
            None,
            &BTreeMap::new(),
            "/workspace",
            false,
        )
        .unwrap();

        assert!(result.is_none());
    }

    // ---------------------------------------------------------------------------
    // compute_dev_dependencies tests
    // ---------------------------------------------------------------------------

    #[test]
    fn compute_dev_dependencies_identifies_dev_only_deps() {
        let build_graph = UnitGraph {
            units: vec![
                make_unit("ws_member#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(1, "dep_a")]),
                make_unit("dep_a#0.2.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
            ],
            roots: vec![0],
        };

        let test_graph = UnitGraph {
            units: vec![
                make_unit("ws_member#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(1, "dep_a")]),
                make_unit("dep_a#0.2.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit(
                    "ws_member#0.1.0",
                    vec![CrateKind::Lib],
                    UnitMode::Test,
                    vec![],
                    vec![(1, "dep_a"), (3, "dev_dep")],
                ),
                make_unit("dev_dep#0.3.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
            ],
            roots: vec![0],
        };

        let metadata = CargoMetadata {
            packages: vec![],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["ws_member#0.1.0".to_string()],
        };

        let meta_by_id = BTreeMap::new();
        let checksums = BTreeMap::new();
        let mut crates = BTreeMap::new();

        // Pre-populate with ws_member and dep_a (from build graph)
        crates.insert(
            "ws_member#0.1.0".to_string(),
            NixCrate {
                crate_name: "ws_member".to_string(),
                version: "0.1.0".to_string(),
                edition: "2021".to_string(),
                sha256: None,
                source: None,
                features: vec![],
                dependencies: vec![NixDep {
                    package_id: "dep_a#0.2.0".to_string(),
                    extern_crate_name: "dep_a".to_string(),
                }],
                build_dependencies: vec![],
                dev_dependencies: vec![],
                proc_macro: false,
                build: None,
                lib_path: None,
                lib_name: None,
                lib_crate_types: vec![],
                crate_bin: vec![],
                links: None,
                authors: vec![],
                description: None,
                homepage: None,
                license: None,
                repository: None,
            },
        );
        crates.insert(
            "dep_a#0.2.0".to_string(),
            NixCrate {
                crate_name: "dep_a".to_string(),
                version: "0.2.0".to_string(),
                edition: "2021".to_string(),
                sha256: None,
                source: None,
                features: vec![],
                dependencies: vec![],
                build_dependencies: vec![],
                dev_dependencies: vec![],
                proc_macro: false,
                build: None,
                lib_path: None,
                lib_name: None,
                lib_crate_types: vec![],
                crate_bin: vec![],
                links: None,
                authors: vec![],
                description: None,
                homepage: None,
                license: None,
                repository: None,
            },
        );

        compute_dev_dependencies(
            &test_graph,
            &build_graph,
            &metadata,
            &meta_by_id,
            &checksums,
            &mut crates,
        )
        .unwrap();

        // dev_dep should have been added to crates
        assert!(crates.contains_key("dev_dep#0.3.0"));

        // ws_member should now have dev_dependencies
        let ws_crate = &crates["ws_member#0.1.0"];
        assert_eq!(ws_crate.dev_dependencies.len(), 1);
        assert_eq!(ws_crate.dev_dependencies[0].package_id, "dev_dep#0.3.0");
        assert_eq!(ws_crate.dev_dependencies[0].extern_crate_name, "dev_dep");
    }

    #[test]
    fn compute_dev_dependencies_adds_dev_only_crates() {
        let build_graph = UnitGraph {
            units: vec![make_unit("ws_member#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![])],
            roots: vec![0],
        };

        let test_graph = UnitGraph {
            units: vec![
                make_unit(
                    "ws_member#0.1.0",
                    vec![CrateKind::Lib],
                    UnitMode::Test,
                    vec![],
                    vec![(1, "test_only")],
                ),
                make_unit("test_only#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
            ],
            roots: vec![0],
        };

        let metadata = CargoMetadata {
            packages: vec![],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["ws_member#0.1.0".to_string()],
        };

        let meta_by_id = BTreeMap::new();
        let checksums = BTreeMap::new();
        let mut crates = BTreeMap::new();

        crates.insert(
            "ws_member#0.1.0".to_string(),
            NixCrate {
                crate_name: "ws_member".to_string(),
                version: "0.1.0".to_string(),
                edition: "2021".to_string(),
                sha256: None,
                source: None,
                features: vec![],
                dependencies: vec![],
                build_dependencies: vec![],
                dev_dependencies: vec![],
                proc_macro: false,
                build: None,
                lib_path: None,
                lib_name: None,
                lib_crate_types: vec![],
                crate_bin: vec![],
                links: None,
                authors: vec![],
                description: None,
                homepage: None,
                license: None,
                repository: None,
            },
        );

        compute_dev_dependencies(
            &test_graph,
            &build_graph,
            &metadata,
            &meta_by_id,
            &checksums,
            &mut crates,
        )
        .unwrap();

        // test_only crate should have been added to the build plan
        assert!(crates.contains_key("test_only#0.1.0"));
        assert_eq!(crates["test_only#0.1.0"].crate_name, "test_only");
    }

    // ---------------------------------------------------------------------------
    // merge tests (end-to-end)
    // ---------------------------------------------------------------------------

    #[test]
    fn merge_simple_workspace() {
        let unit_graph = UnitGraph {
            units: vec![
                make_unit("ws_pkg#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec!["default"], vec![(1, "dep")]),
                make_unit("dep#0.2.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
            ],
            roots: vec![0],
        };

        let metadata = CargoMetadata {
            packages: vec![
                make_meta_pkg("ws_pkg#0.1.0", None, "/workspace/Cargo.toml"),
                make_meta_pkg(
                    "dep#0.2.0",
                    Some("registry+https://github.com/rust-lang/crates.io-index"),
                    "/registry/dep/Cargo.toml",
                ),
            ],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["ws_pkg#0.1.0".to_string()],
        };

        let lock = CargoLock {
            package: Some(vec![make_lock_pkg("dep", "0.2.0", Some("def456"))]),
        };

        let plan = merge(&unit_graph, &metadata, &lock, None, "hash123".to_string(), None, None).unwrap();

        assert_eq!(plan.version, BUILD_PLAN_VERSION);
        assert_eq!(plan.workspace_root, "/workspace");
        assert_eq!(plan.roots.len(), 1);
        assert_eq!(plan.roots[0], "ws_pkg#0.1.0");
        assert_eq!(plan.cargo_lock_hash, "hash123");

        assert_eq!(plan.crates.len(), 2);
        assert!(plan.crates.contains_key("ws_pkg#0.1.0"));
        assert!(plan.crates.contains_key("dep#0.2.0"));

        let ws_crate = &plan.crates["ws_pkg#0.1.0"];
        assert_eq!(ws_crate.crate_name, "ws_pkg");
        assert_eq!(ws_crate.features, vec!["default"]);
        assert_eq!(ws_crate.dependencies.len(), 1);
        assert_eq!(ws_crate.dependencies[0].package_id, "dep#0.2.0");

        let dep_crate = &plan.crates["dep#0.2.0"];
        assert_eq!(dep_crate.crate_name, "dep");
        assert_eq!(dep_crate.sha256, Some("def456".to_string()));
    }

    #[test]
    fn merge_includes_workspace_members_mapping() {
        let unit_graph = UnitGraph {
            units: vec![
                make_unit("member_a#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit("member_b#0.2.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
            ],
            roots: vec![0, 1],
        };

        let metadata = CargoMetadata {
            packages: vec![
                make_meta_pkg("member_a#0.1.0", None, "/workspace/a/Cargo.toml"),
                make_meta_pkg("member_b#0.2.0", None, "/workspace/b/Cargo.toml"),
            ],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["member_a#0.1.0".to_string(), "member_b#0.2.0".to_string()],
        };

        let lock = CargoLock { package: None };

        let plan = merge(&unit_graph, &metadata, &lock, None, "hash".to_string(), None, None).unwrap();

        assert_eq!(plan.workspace_members.len(), 2);
        assert_eq!(plan.workspace_members.get("member_a"), Some(&"member_a#0.1.0".to_string()));
        assert_eq!(plan.workspace_members.get("member_b"), Some(&"member_b#0.2.0".to_string()));
    }

    // ---------------------------------------------------------------------------
    // Workspace filtering tests
    // ---------------------------------------------------------------------------

    #[test]
    fn merge_members_filter_selects_subset() {
        let unit_graph = UnitGraph {
            units: vec![
                make_unit("member_a#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(1, "dep")]),
                make_unit("dep#0.3.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit("member_b#0.2.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
            ],
            roots: vec![0, 2],
        };

        let metadata = CargoMetadata {
            packages: vec![
                make_meta_pkg("member_a#0.1.0", None, "/workspace/a/Cargo.toml"),
                make_meta_pkg("dep#0.3.0", Some("registry+https://github.com/rust-lang/crates.io-index"), "/reg/dep/Cargo.toml"),
                make_meta_pkg("member_b#0.2.0", None, "/workspace/b/Cargo.toml"),
            ],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["member_a#0.1.0".to_string(), "member_b#0.2.0".to_string()],
        };

        let lock = CargoLock { package: None };
        let filter = vec!["member_a".to_string()];

        let plan = merge(&unit_graph, &metadata, &lock, None, "h".to_string(), None, Some(&filter)).unwrap();

        // Only member_a in workspace_members and roots
        assert_eq!(plan.workspace_members.len(), 1);
        assert!(plan.workspace_members.contains_key("member_a"));
        assert!(!plan.workspace_members.contains_key("member_b"));
        assert_eq!(plan.roots.len(), 1);
        assert_eq!(plan.roots[0], "member_a#0.1.0");

        // All crates still present (needed as transitive deps)
        assert_eq!(plan.crates.len(), 3);
    }

    #[test]
    fn merge_members_filter_invalid_name_errors() {
        let unit_graph = UnitGraph {
            units: vec![
                make_unit("member_a#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
            ],
            roots: vec![0],
        };

        let metadata = CargoMetadata {
            packages: vec![
                make_meta_pkg("member_a#0.1.0", None, "/workspace/a/Cargo.toml"),
            ],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["member_a#0.1.0".to_string()],
        };

        let lock = CargoLock { package: None };
        let filter = vec!["nonexistent".to_string()];

        let result = merge(&unit_graph, &metadata, &lock, None, "h".to_string(), None, Some(&filter));

        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("unknown workspace member 'nonexistent'"), "got: {msg}");
        assert!(msg.contains("member_a"), "should list valid members, got: {msg}");
    }
}
