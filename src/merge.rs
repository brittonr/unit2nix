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
    // git deps without @: git+https://host/repo?rev=abc#0.1.0
    // Strip query params (?rev=, ?branch=) before extracting the name.
    let url_path = prefix.split('?').next().unwrap_or(prefix);
    let name = url_path
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
    let source = meta_pkg.map_or_else(
        || infer_source_from_pkg_id(pkg_id),
        |m| {
            parse_source(m.source.as_deref(), &m.manifest_path, workspace_root)
                .unwrap_or_else(|e| {
                    eprintln!("warning: {e:#} for {crate_name}, falling back to pkg_id inference");
                    infer_source_from_pkg_id(pkg_id)
                })
        },
    );

    if source.is_none() && !pkg_id.starts_with("path+") {
        eprintln!(
            "warning: could not determine source for {crate_name} ({pkg_id}), treating as local"
        );
    }

    source
}

/// Shared context for building `NixCrate` values from a unit graph.
///
/// Groups the immutable parameters that `build_nix_crate` needs, reducing
/// the argument count from 8 to 4.
struct MergeContext<'a> {
    unit_graph: &'a UnitGraph,
    unit_pkg_ids: Vec<&'a str>,
    checksums: BTreeMap<(&'a str, &'a str), &'a str>,
    workspace_root: &'a str,
}

/// Build a `NixCrate` from a set of unit graph units for a package.
///
/// Returns `Ok(Some(crate))` on success, `Ok(None)` if the package has no
/// buildable target (should be skipped), or `Err` on parse failures.
fn build_nix_crate(
    ctx: &MergeContext<'_>,
    pkg_id: &str,
    units: &[(usize, &Unit)],
    meta_pkg: Option<&MetadataPackage>,
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

    let (features, host_features) = collect_platform_features(units);
    let proc_macro = primary.target.has_proc_macro();
    let dependencies = collect_dependencies(units, ctx.unit_graph, &ctx.unit_pkg_ids, pkg_id);
    let build_dependencies = collect_build_dependencies(build_script_unit, &ctx.unit_pkg_ids);

    let sha256 = ctx.checksums
        .get(&(crate_name.as_str(), version.as_str()))
        .map(std::string::ToString::to_string);

    let source = resolve_source(pkg_id, &crate_name, meta_pkg, ctx.workspace_root);

    // Crate root directory (from manifest_path, strip Cargo.toml)
    let crate_root = meta_pkg
        .and_then(|m| Path::new(&m.manifest_path).parent())
        .unwrap_or_else(|| Path::new(""));

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
        host_features,
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
/// When `test_unit_graph` is provided (from `cargo test --unit-graph`), it is
/// used as the primary source for all crates — its feature sets already reflect
/// Cargo's feature unification across both normal and dev dependencies, so no
/// post-hoc merging is needed. Dev-only dependencies are classified by
/// comparing Build-mode vs Test-mode units within the test graph itself.
///
/// # Errors
/// Returns an error if package IDs are malformed, dependency references
/// are dangling, or a requested `members_filter` name is invalid.
///
/// # Panics
/// Panics if a root index is out of range in the unit graph (indicates
/// a corrupt unit graph from cargo).
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

    // Use the test graph as the primary source when available.
    //
    // The test graph is a superset of the build graph: it contains every crate
    // from the build graph plus dev-only crates, and its feature sets reflect
    // Cargo's feature unification across both normal and dev dependencies.
    // Building from the superset gives each crate the correct (unified) feature
    // set in a single pass — no post-hoc merging needed.
    //
    // The build graph is then used only to classify which dependencies are
    // dev-only (present in test graph but absent from build graph).
    let primary_graph = test_unit_graph.unwrap_or(unit_graph);

    let ctx = MergeContext {
        unit_graph: primary_graph,
        unit_pkg_ids: primary_graph.units.iter().map(|u| u.pkg_id.as_str()).collect(),
        checksums,
        workspace_root: &metadata.workspace_root,
    };

    // Group units by pkg_id
    let mut pkg_units: BTreeMap<&str, Vec<(usize, &Unit)>> = BTreeMap::new();
    for (idx, unit) in primary_graph.units.iter().enumerate() {
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
            &ctx,
            pkg_id,
            units,
            meta_pkg,
            is_workspace_member,
        )?
        else {
            continue;
        };

        crates.insert(pkg_id.to_string(), nix_crate);
    }

    // When a test graph is available, classify dev-only dependencies on
    // workspace members by comparing Build-mode vs Test-mode units within
    // the test graph. No separate build graph needed — Build-mode units are
    // the normal deps, Test-mode units add dev deps.
    if test_unit_graph.is_some() {
        classify_dev_dependencies(primary_graph, metadata, &mut crates)?;
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

    let (filtered_roots, filtered_workspace_members) =
        apply_members_filter(roots, workspace_members, members_filter)?;

    Ok(NixBuildPlan {
        version: BUILD_PLAN_VERSION,
        workspace_root: metadata.workspace_root.clone(),
        roots: filtered_roots,
        workspace_members: filtered_workspace_members,
        target: target.map(str::to_owned),
        cargo_lock_hash,
        inputs_hash: None, // set by run() after merge
        crates,
    })
}

/// Apply workspace member filtering (--members flag).
///
/// When `filter` is `Some`, validates requested names exist and returns only
/// matching members and their roots. When `None`, passes through unchanged.
fn apply_members_filter(
    roots: Vec<String>,
    workspace_members: BTreeMap<String, String>,
    filter: Option<&[String]>,
) -> Result<(Vec<String>, BTreeMap<String, String>)> {
    let Some(filter) = filter else {
        return Ok((roots, workspace_members));
    };

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
    let member_pkg_ids: HashSet<&str> = filtered_wm.values().map(String::as_str).collect();
    let filtered_roots: Vec<String> = roots
        .into_iter()
        .filter(|r| member_pkg_ids.contains(r.as_str()))
        .collect();

    Ok((filtered_roots, filtered_wm))
}

// ---------------------------------------------------------------------------
// Dependency / feature collection helpers
// ---------------------------------------------------------------------------

/// Collect deduplicated, sorted features across all buildable units for a package.
///
/// The same crate can appear multiple times in the unit graph with different
/// feature sets (e.g., `hashbrown`: once with no features for a proc-macro's
/// host dep, once with `"default"` for a target dep). Nix builds one derivation
/// per crate, so it needs the superset.
///
/// Prefers lib-like units (lib, proc-macro) since they carry the canonical
/// feature set. Falls back to bin units for bin-only crates (no lib target)
/// where features are only present on bin units.
fn collect_features(units: &[(usize, &Unit)]) -> Vec<String> {
    let mut features = BTreeSet::new();
    let has_lib = units
        .iter()
        .any(|(_, u)| u.mode == UnitMode::Build && u.target.has_lib_like());

    for (_, u) in units {
        if u.mode != UnitMode::Build {
            continue;
        }
        let dominated_unit = if has_lib {
            u.target.has_lib_like()
        } else {
            u.target.has_bin()
        };
        if dominated_unit {
            for f in &u.features {
                features.insert(f.clone());
            }
        }
    }
    features.into_iter().collect()
}

/// Collect features split by platform for cross-compilation.
///
/// In cross builds (`--target`), the same crate can appear on both the host
/// (build scripts, proc-macros) and target platforms with different feature
/// sets. For example, `indexmap` might have `["default", "std"]` on the host
/// but `[]` on a no_std kernel target.
///
/// Returns `(target_features, host_features_override)`:
/// - `target_features`: features for the target platform (primary)
/// - `host_features_override`: `Some(features)` when host features differ
///   from target features, `None` when they're the same or no cross build
///
/// For native builds (no `--target`), all units have `platform: None` (host),
/// so this returns the merged features with no override — backward compatible.
fn collect_platform_features(units: &[(usize, &Unit)]) -> (Vec<String>, Option<Vec<String>>) {
    let target_units: Vec<(usize, &Unit)> = units
        .iter()
        .filter(|(_, u)| u.platform.is_some())
        .copied()
        .collect();

    let host_units: Vec<(usize, &Unit)> = units
        .iter()
        .filter(|(_, u)| u.platform.is_none())
        .copied()
        .collect();

    if target_units.is_empty() {
        // Native build or host-only crate: all units are host, no split needed
        (collect_features(units), None)
    } else {
        let target_features = collect_features(&target_units);
        let host_features = if host_units.is_empty() {
            // Target-only crate (e.g., stdlib crate): no host override
            None
        } else {
            let hf = collect_features(&host_units);
            if hf == target_features {
                None
            } else {
                Some(hf)
            }
        };
        (target_features, host_features)
    }
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

/// Classify dev-only dependencies on workspace members.
///
/// All crates are already built from the test graph (which is the superset).
/// Dev deps are identified purely within the test graph by comparing unit
/// modes: Build-mode units carry normal dependencies, Test-mode units add
/// dev dependencies. The diff gives the dev-only set.
fn classify_dev_dependencies(
    test_graph: &UnitGraph,
    metadata: &CargoMetadata,
    crates: &mut BTreeMap<String, NixCrate>,
) -> Result<()> {
    let pkg_ids: Vec<&str> = test_graph.units.iter().map(|u| u.pkg_id.as_str()).collect();

    let ws_ids: HashSet<&str> = metadata
        .workspace_members
        .iter()
        .map(String::as_str)
        .collect();

    // For each workspace member, collect deps from Build-mode units (normal)
    // and Test-mode units (normal + dev). The difference is the dev-only set.
    let mut build_deps: BTreeMap<&str, HashSet<&str>> = BTreeMap::new();
    let mut test_only_deps: BTreeMap<&str, Vec<NixDep>> = BTreeMap::new();

    for unit in &test_graph.units {
        if !ws_ids.contains(unit.pkg_id.as_str()) {
            continue;
        }
        if !unit.target.has_lib_like() && !unit.target.has_bin() {
            continue;
        }

        match unit.mode {
            UnitMode::Build => {
                let entry = build_deps.entry(unit.pkg_id.as_str()).or_default();
                for dep in &unit.dependencies {
                    let dep_pkg_id = pkg_ids[dep.index];
                    if dep_pkg_id != unit.pkg_id.as_str() {
                        entry.insert(dep_pkg_id);
                    }
                }
            }
            UnitMode::Test => {
                let entry = test_only_deps.entry(unit.pkg_id.as_str()).or_default();
                for dep in &unit.dependencies {
                    let dep_unit = &test_graph.units[dep.index];
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
            _ => {}
        }
    }

    for ws_id in &metadata.workspace_members {
        let normal = build_deps.get(ws_id.as_str()).cloned().unwrap_or_default();
        let test_deps = test_only_deps
            .get(ws_id.as_str())
            .cloned()
            .unwrap_or_default();

        let mut seen = HashSet::new();
        let dev_deps: Vec<NixDep> = test_deps
            .into_iter()
            .filter(|dep| {
                !normal.contains(dep.package_id.as_str())
                    && seen.insert((dep.package_id.clone(), dep.extern_crate_name.clone()))
            })
            .collect();

        if !dev_deps.is_empty() {
            if let Some(crate_info) = crates.get_mut(ws_id.as_str()) {
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

    /// Create a minimal `NixCrate` for tests. Override fields after construction.
    fn make_nix_crate(name: &str, version: &str) -> NixCrate {
        NixCrate {
            crate_name: name.to_string(),
            version: version.to_string(),
            edition: "2021".to_string(),
            ..NixCrate::default()
        }
    }

    fn make_unit(
        pkg_id: &str,
        kind: Vec<CrateKind>,
        mode: UnitMode,
        features: Vec<&str>,
        deps: Vec<(usize, &str)>,
    ) -> Unit {
        make_unit_with_platform(pkg_id, kind, mode, features, deps, None)
    }

    fn make_unit_with_platform(
        pkg_id: &str,
        kind: Vec<CrateKind>,
        mode: UnitMode,
        features: Vec<&str>,
        deps: Vec<(usize, &str)>,
        platform: Option<&str>,
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
            platform: platform.map(str::to_string),
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

    #[test]
    fn collect_features_bin_only_crate() {
        // Bin-only crates (no lib target) should collect features from bin units.
        let unit0 = make_unit(
            "pkg#0.1.0",
            vec![CrateKind::Bin],
            UnitMode::Build,
            vec!["ci", "forge"],
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
        assert_eq!(features, vec!["ci", "forge"]);
    }

    #[test]
    fn collect_features_lib_takes_precedence_over_bin() {
        // When both lib and bin exist, only lib features are collected
        // (bin inherits from lib in cargo's model).
        let lib_unit = make_unit(
            "pkg#0.1.0",
            vec![CrateKind::Lib],
            UnitMode::Build,
            vec!["lib_feature"],
            vec![],
        );
        let bin_unit = make_unit(
            "pkg#0.1.0",
            vec![CrateKind::Bin],
            UnitMode::Build,
            vec!["bin_feature"],
            vec![],
        );
        let units = vec![(0, &lib_unit), (1, &bin_unit)];
        let features = collect_features(&units);
        assert_eq!(features, vec!["lib_feature"]);
    }

    // ---------------------------------------------------------------------------
    // collect_platform_features tests
    // ---------------------------------------------------------------------------

    #[test]
    fn collect_platform_features_splits_host_and_target_when_they_differ() {
        let host_unit = make_unit_with_platform(
            "pkg#0.1.0",
            vec![CrateKind::Lib],
            UnitMode::Build,
            vec!["default", "std"],
            vec![],
            None,
        );
        let target_unit = make_unit_with_platform(
            "pkg#0.1.0",
            vec![CrateKind::Lib],
            UnitMode::Build,
            vec!["alloc"],
            vec![],
            Some("aarch64-unknown-none"),
        );

        let (target_features, host_features) =
            collect_platform_features(&[(0, &host_unit), (1, &target_unit)]);

        assert_eq!(target_features, vec!["alloc"]);
        assert_eq!(host_features, Some(vec!["default".to_string(), "std".to_string()]));
    }

    #[test]
    fn collect_platform_features_omits_host_override_when_feature_sets_match() {
        let host_unit = make_unit_with_platform(
            "pkg#0.1.0",
            vec![CrateKind::Lib],
            UnitMode::Build,
            vec!["default", "std"],
            vec![],
            None,
        );
        let target_unit = make_unit_with_platform(
            "pkg#0.1.0",
            vec![CrateKind::Lib],
            UnitMode::Build,
            vec!["default", "std"],
            vec![],
            Some("aarch64-unknown-linux-gnu"),
        );

        let (target_features, host_features) =
            collect_platform_features(&[(0, &host_unit), (1, &target_unit)]);

        assert_eq!(target_features, vec!["default", "std"]);
        assert_eq!(host_features, None);
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
        let mut pkg_a = make_nix_crate("pkg_a", "0.1.0");
        pkg_a.dependencies = vec![NixDep {
            package_id: "pkg_b#0.2.0".to_string(),
            extern_crate_name: "pkg_b".to_string(),
        }];
        crates.insert("pkg_a#0.1.0".to_string(), pkg_a);
        crates.insert("pkg_b#0.2.0".to_string(), make_nix_crate("pkg_b", "0.2.0"));

        let result = validate_references(&crates);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_references_fails_with_dangling_refs() {
        let mut crates = BTreeMap::new();
        let mut pkg_a = make_nix_crate("pkg_a", "0.1.0");
        pkg_a.dependencies = vec![NixDep {
            package_id: "missing#0.9.0".to_string(),
            extern_crate_name: "missing".to_string(),
        }];
        crates.insert("pkg_a#0.1.0".to_string(), pkg_a);

        let result = validate_references(&crates);
        assert!(result.is_err());
    }

    #[test]
    fn validate_references_checks_build_dependencies() {
        let mut crates = BTreeMap::new();
        let mut pkg_a = make_nix_crate("pkg_a", "0.1.0");
        pkg_a.build_dependencies = vec![NixDep {
            package_id: "missing-build#0.9.0".to_string(),
            extern_crate_name: "missing_build".to_string(),
        }];
        crates.insert("pkg_a#0.1.0".to_string(), pkg_a);

        let err = validate_references(&crates).unwrap_err().to_string();
        assert!(err.contains("reference crates not in the build plan"), "got: {err}");
    }

    #[test]
    fn validate_references_checks_dev_dependencies() {
        let mut crates = BTreeMap::new();
        let mut pkg_a = make_nix_crate("pkg_a", "0.1.0");
        pkg_a.dev_dependencies = vec![NixDep {
            package_id: "missing-dev#0.9.0".to_string(),
            extern_crate_name: "missing_dev".to_string(),
        }];
        crates.insert("pkg_a#0.1.0".to_string(), pkg_a);

        let err = validate_references(&crates).unwrap_err().to_string();
        assert!(err.contains("reference crates not in the build plan"), "got: {err}");
    }

    // ---------------------------------------------------------------------------
    // build_nix_crate tests
    // ---------------------------------------------------------------------------

    /// Build a `MergeContext` from a `UnitGraph` for tests.
    fn make_ctx(unit_graph: &UnitGraph) -> MergeContext<'_> {
        MergeContext {
            unit_graph,
            unit_pkg_ids: unit_graph.units.iter().map(|u| u.pkg_id.as_str()).collect(),
            checksums: BTreeMap::new(),
            workspace_root: "/workspace",
        }
    }

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
        let mut ctx = make_ctx(&unit_graph);
        ctx.checksums.insert(("serde", "1.0.0"), "abc123");
        let units = vec![(0, &unit_graph.units[0])];

        let meta_pkg = make_meta_pkg(
            pkg_id,
            Some("registry+https://github.com/rust-lang/crates.io-index"),
            "/path/to/Cargo.toml",
        );

        let result = build_nix_crate(
            &ctx,
            pkg_id,
            &units,
            Some(&meta_pkg),
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
        let ctx = make_ctx(&unit_graph);
        let units = vec![(0, &unit_graph.units[0])];

        let meta_pkg = make_meta_pkg(pkg_id, None, "/home/user/proj/Cargo.toml");

        let result = build_nix_crate(
            &ctx,
            pkg_id,
            &units,
            Some(&meta_pkg),
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
        let ctx = make_ctx(&unit_graph);
        let units = vec![(0, &unit_graph.units[0])];

        let result = build_nix_crate(
            &ctx,
            pkg_id,
            &units,
            None,
            false,
        )
        .unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn build_nix_crate_proc_macro_sets_proc_macro_flag() {
        let pkg_id = "proc_macro_dep#0.1.0";
        let unit_graph = UnitGraph {
            units: vec![make_unit(
                pkg_id,
                vec![CrateKind::ProcMacro],
                UnitMode::Build,
                vec![],
                vec![],
            )],
            roots: vec![0],
        };
        let ctx = make_ctx(&unit_graph);
        let units = vec![(0, &unit_graph.units[0])];

        let result = build_nix_crate(&ctx, pkg_id, &units, None, false).unwrap();
        let crate_info = result.expect("proc macro should build");
        assert!(crate_info.proc_macro);
        assert_eq!(crate_info.crate_name, "proc_macro_dep");
    }

    // ---------------------------------------------------------------------------
    // classify_dev_dependencies tests
    // ---------------------------------------------------------------------------

    #[test]
    fn classify_dev_dependencies_from_single_graph() {
        // The test graph has Build-mode and Test-mode units for the workspace
        // member. Dev deps are those on Test-mode units but not Build-mode.
        let test_graph = UnitGraph {
            units: vec![
                // 0: ws_member Build-mode (normal deps)
                make_unit("ws_member#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(1, "dep_a")]),
                // 1: dep_a (normal dep)
                make_unit("dep_a#0.2.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                // 2: ws_member Test-mode (normal + dev deps)
                make_unit(
                    "ws_member#0.1.0",
                    vec![CrateKind::Lib],
                    UnitMode::Test,
                    vec![],
                    vec![(1, "dep_a"), (3, "dev_dep")],
                ),
                // 3: dev_dep (dev-only)
                make_unit("dev_dep#0.3.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
            ],
            roots: vec![0],
        };

        let metadata = CargoMetadata {
            packages: vec![],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["ws_member#0.1.0".to_string()],
        };

        // Pre-populate crates as merge() would (built from test graph)
        let mut crates = BTreeMap::new();
        let mut ws_member = make_nix_crate("ws_member", "0.1.0");
        ws_member.dependencies = vec![NixDep {
            package_id: "dep_a#0.2.0".to_string(),
            extern_crate_name: "dep_a".to_string(),
        }];
        crates.insert("ws_member#0.1.0".to_string(), ws_member);
        crates.insert("dep_a#0.2.0".to_string(), make_nix_crate("dep_a", "0.2.0"));
        crates.insert("dev_dep#0.3.0".to_string(), make_nix_crate("dev_dep", "0.3.0"));

        classify_dev_dependencies(&test_graph, &metadata, &mut crates).unwrap();

        let ws_crate = &crates["ws_member#0.1.0"];
        assert_eq!(ws_crate.dev_dependencies.len(), 1);
        assert_eq!(ws_crate.dev_dependencies[0].package_id, "dev_dep#0.3.0");
        assert_eq!(ws_crate.dev_dependencies[0].extern_crate_name, "dev_dep");
    }

    #[test]
    fn classify_dev_dependencies_skips_run_custom_build_in_test_units() {
        let test_graph = UnitGraph {
            units: vec![
                make_unit("ws_member#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit(
                    "build_helper#0.1.0",
                    vec![CrateKind::CustomBuild],
                    UnitMode::RunCustomBuild,
                    vec![],
                    vec![],
                ),
                make_unit(
                    "ws_member#0.1.0",
                    vec![CrateKind::Lib],
                    UnitMode::Test,
                    vec![],
                    vec![(1, "build_helper")],
                ),
            ],
            roots: vec![0],
        };

        let metadata = CargoMetadata {
            packages: vec![],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["ws_member#0.1.0".to_string()],
        };

        let mut crates = BTreeMap::new();
        crates.insert("ws_member#0.1.0".to_string(), make_nix_crate("ws_member", "0.1.0"));
        crates.insert("build_helper#0.1.0".to_string(), make_nix_crate("build_helper", "0.1.0"));

        classify_dev_dependencies(&test_graph, &metadata, &mut crates).unwrap();
        assert!(crates["ws_member#0.1.0"].dev_dependencies.is_empty());
    }

    #[test]
    fn classify_dev_dependencies_deduplicates_same_dev_dep_within_member() {
        let test_graph = UnitGraph {
            units: vec![
                make_unit("ws_member#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit("dev_dep#0.3.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit(
                    "ws_member#0.1.0",
                    vec![CrateKind::Lib],
                    UnitMode::Test,
                    vec![],
                    vec![(1, "dev_dep")],
                ),
                make_unit(
                    "ws_member#0.1.0",
                    vec![CrateKind::Bin],
                    UnitMode::Test,
                    vec![],
                    vec![(1, "dev_dep")],
                ),
            ],
            roots: vec![0],
        };

        let metadata = CargoMetadata {
            packages: vec![],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["ws_member#0.1.0".to_string()],
        };

        let mut crates = BTreeMap::new();
        crates.insert("ws_member#0.1.0".to_string(), make_nix_crate("ws_member", "0.1.0"));
        crates.insert("dev_dep#0.3.0".to_string(), make_nix_crate("dev_dep", "0.3.0"));

        classify_dev_dependencies(&test_graph, &metadata, &mut crates).unwrap();
        let deps = &crates["ws_member#0.1.0"].dev_dependencies;
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].package_id, "dev_dep#0.3.0");
    }

    #[test]
    fn classify_dev_dependencies_resets_dedup_per_workspace_member() {
        let test_graph = UnitGraph {
            units: vec![
                make_unit("member_a#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit("member_b#0.2.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit("shared_dev#0.3.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit(
                    "member_a#0.1.0",
                    vec![CrateKind::Lib],
                    UnitMode::Test,
                    vec![],
                    vec![(2, "shared_dev")],
                ),
                make_unit(
                    "member_b#0.2.0",
                    vec![CrateKind::Lib],
                    UnitMode::Test,
                    vec![],
                    vec![(2, "shared_dev")],
                ),
            ],
            roots: vec![0, 1],
        };

        let metadata = CargoMetadata {
            packages: vec![],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["member_a#0.1.0".to_string(), "member_b#0.2.0".to_string()],
        };

        let mut crates = BTreeMap::new();
        crates.insert("member_a#0.1.0".to_string(), make_nix_crate("member_a", "0.1.0"));
        crates.insert("member_b#0.2.0".to_string(), make_nix_crate("member_b", "0.2.0"));
        crates.insert("shared_dev#0.3.0".to_string(), make_nix_crate("shared_dev", "0.3.0"));

        classify_dev_dependencies(&test_graph, &metadata, &mut crates).unwrap();
        assert_eq!(crates["member_a#0.1.0"].dev_dependencies.len(), 1);
        assert_eq!(crates["member_b#0.2.0"].dev_dependencies.len(), 1);
        assert_eq!(crates["member_a#0.1.0"].dev_dependencies[0].package_id, "shared_dev#0.3.0");
        assert_eq!(crates["member_b#0.2.0"].dev_dependencies[0].package_id, "shared_dev#0.3.0");
    }

    #[test]
    fn classify_dev_dependencies_treats_test_only_member_deps_as_dev_deps() {
        let test_graph = UnitGraph {
            units: vec![
                make_unit("test_only_member#0.1.0", vec![CrateKind::Lib], UnitMode::Test, vec![], vec![(1, "dev_dep")]),
                make_unit("dev_dep#0.3.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
            ],
            roots: vec![0],
        };

        let metadata = CargoMetadata {
            packages: vec![],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["test_only_member#0.1.0".to_string()],
        };

        let mut crates = BTreeMap::new();
        crates.insert("test_only_member#0.1.0".to_string(), make_nix_crate("test_only_member", "0.1.0"));
        crates.insert("dev_dep#0.3.0".to_string(), make_nix_crate("dev_dep", "0.3.0"));

        classify_dev_dependencies(&test_graph, &metadata, &mut crates).unwrap();
        let deps = &crates["test_only_member#0.1.0"].dev_dependencies;
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].package_id, "dev_dep#0.3.0");
    }

    // ---------------------------------------------------------------------------
    // merge with test graph (end-to-end)
    // ---------------------------------------------------------------------------

    /// Dev-only crates appear in the build plan when a test graph is provided.
    #[test]
    fn merge_with_test_graph_includes_dev_only_crates() {
        let build_graph = UnitGraph {
            units: vec![make_unit("ws#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![])],
            roots: vec![0],
        };
        let test_graph = UnitGraph {
            units: vec![
                make_unit("ws#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit(
                    "ws#0.1.0",
                    vec![CrateKind::Lib],
                    UnitMode::Test,
                    vec![],
                    vec![(2, "test_only")],
                ),
                make_unit("test_only#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
            ],
            roots: vec![0],
        };

        let metadata = CargoMetadata {
            packages: vec![make_meta_pkg("ws#0.1.0", None, "/workspace/Cargo.toml")],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["ws#0.1.0".to_string()],
        };
        let lock = CargoLock { package: None };

        let plan = merge(&build_graph, &metadata, &lock, None, "h".to_string(), Some(&test_graph), None).unwrap();

        assert!(plan.crates.contains_key("test_only#0.1.0"));
        assert_eq!(plan.crates["test_only#0.1.0"].crate_name, "test_only");
    }

    /// Regression test: crates in both build and test graphs get the test
    /// graph's feature set (the superset) because merge() builds from the
    /// test graph as primary source.
    ///
    /// Real-world scenario: `zerocopy` has `["simd"]` in the build graph but
    /// `["derive", "simd", "zerocopy-derive"]` in the test graph because
    /// `half` (a dev-dep via criterion) requests the `derive` feature.
    #[test]
    fn merge_with_test_graph_uses_superset_features() {
        let zc_id = "registry+https://github.com/rust-lang/crates.io-index#zerocopy@0.8.40";
        let zc_derive_id = "registry+https://github.com/rust-lang/crates.io-index#zerocopy-derive@0.8.40";

        let build_graph = UnitGraph {
            units: vec![
                make_unit("ws#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(1, "zerocopy")]),
                make_unit(zc_id, vec![CrateKind::Lib], UnitMode::Build, vec!["simd"], vec![]),
            ],
            roots: vec![0],
        };

        let test_graph = UnitGraph {
            units: vec![
                make_unit("ws#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(1, "zerocopy")]),
                make_unit(
                    zc_id,
                    vec![CrateKind::Lib],
                    UnitMode::Build,
                    vec!["derive", "simd", "zerocopy-derive"],
                    vec![(2, "zerocopy_derive")],
                ),
                make_unit(zc_derive_id, vec![CrateKind::ProcMacro], UnitMode::Build, vec![], vec![]),
                make_unit(
                    "ws#0.1.0",
                    vec![CrateKind::Lib],
                    UnitMode::Test,
                    vec![],
                    vec![(1, "zerocopy"), (4, "half")],
                ),
                make_unit("half#2.0.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(1, "zerocopy")]),
            ],
            roots: vec![0],
        };

        let metadata = CargoMetadata {
            packages: vec![make_meta_pkg("ws#0.1.0", None, "/workspace/Cargo.toml")],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["ws#0.1.0".to_string()],
        };
        let lock = CargoLock { package: None };

        let plan = merge(&build_graph, &metadata, &lock, None, "h".to_string(), Some(&test_graph), None).unwrap();

        // zerocopy gets the test graph's superset features
        let zc = &plan.crates[zc_id];
        assert_eq!(zc.features, vec!["derive", "simd", "zerocopy-derive"]);

        // zerocopy-derive is a dependency of zerocopy (from the derive feature)
        let zc_dep_ids: Vec<&str> = zc.dependencies.iter().map(|d| d.package_id.as_str()).collect();
        assert!(zc_dep_ids.contains(&zc_derive_id));

        // zerocopy-derive and half both in the plan
        assert!(plan.crates.contains_key(zc_derive_id));
        assert!(plan.crates.contains_key("half#2.0.0"));

        // half is classified as a dev-dep of the workspace member
        let ws = &plan.crates["ws#0.1.0"];
        let dev_dep_ids: Vec<&str> = ws.dev_dependencies.iter().map(|d| d.package_id.as_str()).collect();
        assert!(dev_dep_ids.contains(&"half#2.0.0"));
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
    fn merge_target_propagates_to_plan() {
        let unit_graph = UnitGraph {
            units: vec![
                make_unit("ws_pkg#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
            ],
            roots: vec![0],
        };

        let metadata = CargoMetadata {
            packages: vec![
                make_meta_pkg("ws_pkg#0.1.0", None, "/workspace/Cargo.toml"),
            ],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["ws_pkg#0.1.0".to_string()],
        };

        let lock = CargoLock { package: None };

        // With target set
        let plan = merge(
            &unit_graph, &metadata, &lock,
            Some("aarch64-unknown-linux-gnu"),
            "hash".to_string(), None, None,
        ).unwrap();
        assert_eq!(plan.target, Some("aarch64-unknown-linux-gnu".to_string()));

        // Without target
        let plan_no_target = merge(
            &unit_graph, &metadata, &lock,
            None,
            "hash".to_string(), None, None,
        ).unwrap();
        assert_eq!(plan_no_target.target, None);
    }

    #[test]
    fn merge_cross_target_records_host_feature_override() {
        let pkg_id = "dep#0.2.0";
        let unit_graph = UnitGraph {
            units: vec![
                make_unit("ws_pkg#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(1, "dep")]),
                make_unit_with_platform(
                    pkg_id,
                    vec![CrateKind::Lib],
                    UnitMode::Build,
                    vec!["default", "std"],
                    vec![],
                    None,
                ),
                make_unit_with_platform(
                    pkg_id,
                    vec![CrateKind::Lib],
                    UnitMode::Build,
                    vec!["alloc"],
                    vec![],
                    Some("aarch64-unknown-none"),
                ),
            ],
            roots: vec![0],
        };

        let metadata = CargoMetadata {
            packages: vec![
                make_meta_pkg("ws_pkg#0.1.0", None, "/workspace/Cargo.toml"),
                make_meta_pkg(pkg_id, Some("registry+https://github.com/rust-lang/crates.io-index"), "/registry/dep/Cargo.toml"),
            ],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["ws_pkg#0.1.0".to_string()],
        };

        let plan = merge(
            &unit_graph,
            &metadata,
            &CargoLock { package: None },
            Some("aarch64-unknown-none"),
            "hash".to_string(),
            None,
            None,
        ).unwrap();

        let dep = &plan.crates[pkg_id];
        assert_eq!(dep.features, vec!["alloc"]);
        assert_eq!(dep.host_features, Some(vec!["default".to_string(), "std".to_string()]));
    }

    #[test]
    fn merge_plan_is_reference_closed_after_members_filter() {
        let unit_graph = UnitGraph {
            units: vec![
                make_unit("member_a#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(1, "dep")]),
                make_unit("dep#0.3.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(2, "leaf")]),
                make_unit("leaf#0.4.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit("member_b#0.2.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
            ],
            roots: vec![0, 3],
        };

        let metadata = CargoMetadata {
            packages: vec![
                make_meta_pkg("member_a#0.1.0", None, "/workspace/a/Cargo.toml"),
                make_meta_pkg("dep#0.3.0", Some("registry+https://github.com/rust-lang/crates.io-index"), "/reg/dep/Cargo.toml"),
                make_meta_pkg("leaf#0.4.0", Some("registry+https://github.com/rust-lang/crates.io-index"), "/reg/leaf/Cargo.toml"),
                make_meta_pkg("member_b#0.2.0", None, "/workspace/b/Cargo.toml"),
            ],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["member_a#0.1.0".to_string(), "member_b#0.2.0".to_string()],
        };

        let filter = vec!["member_a".to_string()];
        let plan = merge(
            &unit_graph,
            &metadata,
            &CargoLock { package: None },
            None,
            "hash".to_string(),
            None,
            Some(&filter),
        ).unwrap();

        validate_references(&plan.crates).unwrap();
        let root_ids: BTreeSet<_> = plan.roots.iter().cloned().collect();
        let member_ids: BTreeSet<_> = plan.workspace_members.values().cloned().collect();
        assert!(root_ids.is_subset(&member_ids), "roots should be selected workspace members");
        assert!(root_ids.iter().all(|id| plan.crates.contains_key(id)), "roots must exist in crate map");
        assert!(plan.crates.contains_key("dep#0.3.0"), "transitive dep should stay in crate map");
        assert!(plan.crates.contains_key("leaf#0.4.0"), "transitive dep closure should stay intact");
    }

    #[test]
    fn merge_test_graph_only_assigns_dev_dependencies_to_workspace_members() {
        let build_graph = UnitGraph {
            units: vec![make_unit("ws#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(1, "dep_a")])],
            roots: vec![0],
        };
        let test_graph = UnitGraph {
            units: vec![
                make_unit("ws#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(1, "dep_a")]),
                make_unit("dep_a#0.2.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit(
                    "ws#0.1.0",
                    vec![CrateKind::Lib],
                    UnitMode::Test,
                    vec![],
                    vec![(1, "dep_a"), (3, "dev_only")],
                ),
                make_unit("dev_only#0.3.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
            ],
            roots: vec![0],
        };

        let metadata = CargoMetadata {
            packages: vec![make_meta_pkg("ws#0.1.0", None, "/workspace/Cargo.toml")],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["ws#0.1.0".to_string()],
        };

        let plan = merge(
            &build_graph,
            &metadata,
            &CargoLock { package: None },
            None,
            "hash".to_string(),
            Some(&test_graph),
            None,
        ).unwrap();

        assert_eq!(plan.crates["ws#0.1.0"].dev_dependencies.len(), 1);
        assert!(plan.crates["dep_a#0.2.0"].dev_dependencies.is_empty());
        assert!(plan.crates["dev_only#0.3.0"].dev_dependencies.is_empty());
        validate_references(&plan.crates).unwrap();
    }

    #[test]
    fn merge_recovers_from_malformed_metadata_source_via_pkg_id_inference() {
        let dep_id = "registry+https://github.com/rust-lang/crates.io-index#dep@0.2.0";
        let unit_graph = UnitGraph {
            units: vec![
                make_unit("ws#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(1, "dep")]),
                make_unit(dep_id, vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
            ],
            roots: vec![0],
        };

        let metadata = CargoMetadata {
            packages: vec![
                make_meta_pkg("ws#0.1.0", None, "/workspace/Cargo.toml"),
                make_meta_pkg(dep_id, Some("sparse+https://example.invalid/index"), "/registry/dep/Cargo.toml"),
            ],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["ws#0.1.0".to_string()],
        };

        let plan = merge(
            &unit_graph,
            &metadata,
            &CargoLock { package: None },
            None,
            "hash".to_string(),
            None,
            None,
        ).unwrap();

        assert!(matches!(plan.crates[dep_id].source, Some(NixSource::CratesIo)));
        validate_references(&plan.crates).unwrap();
    }

    #[test]
    fn merge_build_script_dependencies_stay_separate_from_normal_dependencies() {
        let mut lib_unit = make_unit(
            "ws#0.1.0",
            vec![CrateKind::Lib],
            UnitMode::Build,
            vec![],
            vec![(2, "runtime_dep")],
        );
        lib_unit.target.src_path = "/workspace/src/lib.rs".to_string();

        let mut build_script_unit = make_unit(
            "ws#0.1.0",
            vec![CrateKind::CustomBuild],
            UnitMode::Build,
            vec![],
            vec![(3, "build_dep")],
        );
        build_script_unit.target.src_path = "/workspace/build-support/custom-build.rs".to_string();

        let unit_graph = UnitGraph {
            units: vec![
                lib_unit,
                build_script_unit,
                make_unit(
                    "runtime_dep#0.2.0",
                    vec![CrateKind::Lib],
                    UnitMode::Build,
                    vec![],
                    vec![],
                ),
                make_unit(
                    "build_dep#0.3.0",
                    vec![CrateKind::Lib],
                    UnitMode::Build,
                    vec![],
                    vec![],
                ),
            ],
            roots: vec![0],
        };

        let metadata = CargoMetadata {
            packages: vec![
                make_meta_pkg("ws#0.1.0", None, "/workspace/Cargo.toml"),
                make_meta_pkg("runtime_dep#0.2.0", Some("registry+https://github.com/rust-lang/crates.io-index"), "/reg/runtime/Cargo.toml"),
                make_meta_pkg("build_dep#0.3.0", Some("registry+https://github.com/rust-lang/crates.io-index"), "/reg/build/Cargo.toml"),
            ],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["ws#0.1.0".to_string()],
        };

        let plan = merge(
            &unit_graph,
            &metadata,
            &CargoLock { package: None },
            None,
            "hash".to_string(),
            None,
            None,
        ).unwrap();

        let ws = &plan.crates["ws#0.1.0"];
        assert_eq!(ws.dependencies.len(), 1);
        assert_eq!(ws.dependencies[0].package_id, "runtime_dep#0.2.0");
        assert_eq!(ws.build_dependencies.len(), 1);
        assert_eq!(ws.build_dependencies[0].package_id, "build_dep#0.3.0");
        assert_eq!(ws.build, Some("build-support/custom-build.rs".to_string()));
        validate_references(&plan.crates).unwrap();
    }

    #[test]
    fn merge_bin_only_workspace_member_keeps_bin_target_metadata() {
        let mut bin_unit = make_unit(
            "bin_only#0.1.0",
            vec![CrateKind::Bin],
            UnitMode::Build,
            vec!["cli"],
            vec![],
        );
        bin_unit.target.name = "bin-only".to_string();
        bin_unit.target.src_path = "/workspace/src/bin/main.rs".to_string();

        let unit_graph = UnitGraph {
            units: vec![bin_unit],
            roots: vec![0],
        };

        let metadata = CargoMetadata {
            packages: vec![make_meta_pkg("bin_only#0.1.0", None, "/workspace/Cargo.toml")],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["bin_only#0.1.0".to_string()],
        };

        let plan = merge(
            &unit_graph,
            &metadata,
            &CargoLock { package: None },
            None,
            "hash".to_string(),
            None,
            None,
        ).unwrap();

        let bin = &plan.crates["bin_only#0.1.0"];
        assert_eq!(bin.features, vec!["cli"]);
        assert_eq!(bin.lib_path, None);
        assert_eq!(bin.lib_name, None);
        assert_eq!(bin.crate_bin.len(), 1);
        assert_eq!(bin.crate_bin[0].name, "bin-only");
        assert_eq!(bin.crate_bin[0].path, "src/bin/main.rs");
    }

    #[test]
    fn merge_members_filter_with_test_graph_keeps_selected_member_dev_dependencies() {
        let build_graph = UnitGraph {
            units: vec![
                make_unit("member_a#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit("member_b#0.2.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
            ],
            roots: vec![0, 1],
        };
        let test_graph = UnitGraph {
            units: vec![
                make_unit("member_a#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit("member_b#0.2.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit(
                    "member_a#0.1.0",
                    vec![CrateKind::Lib],
                    UnitMode::Test,
                    vec![],
                    vec![(4, "dev_a")],
                ),
                make_unit(
                    "member_b#0.2.0",
                    vec![CrateKind::Lib],
                    UnitMode::Test,
                    vec![],
                    vec![(5, "dev_b")],
                ),
                make_unit("dev_a#0.3.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit("dev_b#0.4.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
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
        let filter = vec!["member_a".to_string()];

        let plan = merge(
            &build_graph,
            &metadata,
            &CargoLock { package: None },
            None,
            "hash".to_string(),
            Some(&test_graph),
            Some(&filter),
        ).unwrap();

        assert_eq!(plan.workspace_members.len(), 1);
        assert_eq!(plan.workspace_members.get("member_a"), Some(&"member_a#0.1.0".to_string()));
        assert_eq!(plan.roots, vec!["member_a#0.1.0".to_string()]);
        assert_eq!(plan.crates["member_a#0.1.0"].dev_dependencies.len(), 1);
        assert_eq!(plan.crates["member_a#0.1.0"].dev_dependencies[0].package_id, "dev_a#0.3.0");
        validate_references(&plan.crates).unwrap();
    }

    #[test]
    fn merge_matrix_preserves_internal_crate_graph_across_filter_and_test_graph_variants() {
        let build_graph = UnitGraph {
            units: vec![
                make_unit("member_a#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(2, "shared")]),
                make_unit("member_b#0.2.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(2, "shared")]),
                make_unit("shared#0.3.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
            ],
            roots: vec![0, 1],
        };
        let test_graph = UnitGraph {
            units: vec![
                make_unit("member_a#0.1.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(2, "shared")]),
                make_unit("member_b#0.2.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![(2, "shared")]),
                make_unit("shared#0.3.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit("dev_a#0.4.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit("dev_b#0.5.0", vec![CrateKind::Lib], UnitMode::Build, vec![], vec![]),
                make_unit(
                    "member_a#0.1.0",
                    vec![CrateKind::Lib],
                    UnitMode::Test,
                    vec![],
                    vec![(2, "shared"), (3, "dev_a")],
                ),
                make_unit(
                    "member_b#0.2.0",
                    vec![CrateKind::Lib],
                    UnitMode::Test,
                    vec![],
                    vec![(2, "shared"), (4, "dev_b")],
                ),
            ],
            roots: vec![0, 1],
        };
        let metadata = CargoMetadata {
            packages: vec![
                make_meta_pkg("member_a#0.1.0", None, "/workspace/a/Cargo.toml"),
                make_meta_pkg("member_b#0.2.0", None, "/workspace/b/Cargo.toml"),
                make_meta_pkg("shared#0.3.0", Some("registry+https://github.com/rust-lang/crates.io-index"), "/reg/shared/Cargo.toml"),
            ],
            workspace_root: "/workspace".to_string(),
            workspace_members: vec!["member_a#0.1.0".to_string(), "member_b#0.2.0".to_string()],
        };

        for use_test_graph in [false, true] {
            for filter in [None, Some(vec!["member_a".to_string()])] {
                let plan = merge(
                    &build_graph,
                    &metadata,
                    &CargoLock { package: None },
                    None,
                    "hash".to_string(),
                    use_test_graph.then_some(&test_graph),
                    filter.as_deref(),
                ).unwrap();

                validate_references(&plan.crates).unwrap();
                assert!(plan.crates.contains_key("member_a#0.1.0"));
                assert!(plan.crates.contains_key("member_b#0.2.0"));
                assert!(plan.crates.contains_key("shared#0.3.0"));

                if let Some(filter) = &filter {
                    assert_eq!(plan.workspace_members.len(), 1, "filter={filter:?} test_graph={use_test_graph}");
                    assert_eq!(plan.workspace_members.get("member_a"), Some(&"member_a#0.1.0".to_string()));
                    assert_eq!(plan.roots, vec!["member_a#0.1.0".to_string()]);
                } else {
                    assert_eq!(plan.workspace_members.len(), 2, "test_graph={use_test_graph}");
                    assert_eq!(plan.roots.len(), 2, "test_graph={use_test_graph}");
                }

                if use_test_graph {
                    assert_eq!(plan.crates["member_a#0.1.0"].dev_dependencies.len(), 1);
                    assert_eq!(plan.crates["member_b#0.2.0"].dev_dependencies.len(), 1);
                } else {
                    assert!(plan.crates["member_a#0.1.0"].dev_dependencies.is_empty());
                    assert!(plan.crates["member_b#0.2.0"].dev_dependencies.is_empty());
                }
            }
        }
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
