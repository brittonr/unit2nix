use std::collections::BTreeMap;

use anyhow::{bail, Result};

use crate::unit_graph::{UnitGraph, Unit};
use crate::metadata::{CargoMetadata, CargoLock, MetadataPackage};
use crate::output::{NixBuildPlan, NixCrate, NixDep, NixBinTarget};
use crate::source::{parse_source, infer_source_from_pkg_id};

/// Returns true if the target kind represents a library (lib, rlib, cdylib, etc).
fn is_lib_kind(kind: &[String]) -> bool {
    kind.iter()
        .any(|k| matches!(k.as_str(), "lib" | "rlib" | "cdylib" | "dylib" | "staticlib"))
}

fn is_proc_macro(kind: &[String]) -> bool {
    kind.iter().any(|k| k == "proc-macro")
}

fn is_bin(kind: &[String]) -> bool {
    kind.iter().any(|k| k == "bin")
}

fn is_custom_build(kind: &[String]) -> bool {
    kind.iter().any(|k| k == "custom-build")
}

/// Extract name@version from a pkg_id string.
/// Formats: "registry+...#name@version" or "path+file:///...#version"
pub fn parse_pkg_id(pkg_id: &str) -> (String, String) {
    if let Some((_prefix, fragment)) = pkg_id.rsplit_once('#') {
        if let Some((name, version)) = fragment.rsplit_once('@') {
            return (name.to_string(), version.to_string());
        }
        // path deps: fragment is just the version, name is in the path
        // e.g., path+file:///home/user/proj/crates/foo#0.1.0
        let path_part = pkg_id.split('#').next().unwrap_or("");
        let name = path_part
            .rsplit('/')
            .next()
            .unwrap_or("unknown")
            .to_string();
        return (name, fragment.to_string());
    }
    ("unknown".to_string(), "0.0.0".to_string())
}

pub fn merge(unit_graph: &UnitGraph, metadata: &CargoMetadata, lock: &CargoLock, target: Option<&str>, cargo_lock_hash: String) -> Result<NixBuildPlan> {
    // Index metadata packages by their id
    let meta_by_id: BTreeMap<&str, &MetadataPackage> = metadata
        .packages
        .iter()
        .map(|p| (p.id.as_str(), p))
        .collect();

    // Index Cargo.lock checksums by (name, version, source)
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
        // Find the primary lib unit (lib/rlib/cdylib/dylib/staticlib or proc-macro)
        let lib_unit: Option<&(usize, &Unit)> = units
            .iter()
            .find(|(_, u)| u.mode == "build" && is_lib_kind(&u.target.kind))
            .or_else(|| {
                units
                    .iter()
                    .find(|(_, u)| u.mode == "build" && is_proc_macro(&u.target.kind))
            });

        let bin_units: Vec<&(usize, &Unit)> = units
            .iter()
            .filter(|(_, u)| u.mode == "build" && is_bin(&u.target.kind))
            .collect();

        let build_script_unit = units
            .iter()
            .find(|(_, u)| u.mode == "build" && is_custom_build(&u.target.kind));

        // Skip run-custom-build units (they're internal)
        // Skip units that are only run-custom-build with no lib/bin
        let primary_unit = lib_unit.or_else(|| bin_units.first().copied());
        let primary_unit = match primary_unit {
            Some(u) => u,
            None => continue, // No buildable target
        };

        let (_, primary) = primary_unit;
        let (crate_name, version) = parse_pkg_id(pkg_id);

        // Look up metadata for extra info
        let meta_pkg = meta_by_id.get(pkg_id);

        // Features: union across all lib-like units for this package.
        // The same crate can appear multiple times in the unit graph with
        // different feature sets (e.g., hashbrown: once with no features for
        // a proc-macro's host dep, once with "default" for a target dep).
        // Nix builds one derivation per crate, so it needs the superset.
        let features = {
            let mut all_features: Vec<String> = Vec::new();
            for (_, u) in units {
                if u.mode == "build" && (is_lib_kind(&u.target.kind)
                    || is_proc_macro(&u.target.kind))
                {
                    for f in &u.features {
                        if !all_features.contains(f) {
                            all_features.push(f.clone());
                        }
                    }
                }
            }
            all_features.sort();
            all_features
        };

        // Is this a proc-macro?
        let proc_macro = is_proc_macro(&primary.target.kind);

        // Normal dependencies: union across the primary unit and all lib-like
        // units for this package. Different feature variants may pull in
        // different deps; bin-only crates have deps on the bin unit only.
        let dependencies: Vec<NixDep> = {
            let mut deps = Vec::new();
            let mut seen = std::collections::HashSet::new();
            let dep_units = units.iter().filter(|(_, u)| {
                u.mode == "build"
                    && (is_lib_kind(&u.target.kind)
                        || is_proc_macro(&u.target.kind)
                        || is_bin(&u.target.kind))
            });
            for (_, u) in dep_units {
                for dep in &u.dependencies {
                    let dep_unit = &unit_graph.units[dep.index];
                    // Skip self-references (bin → lib within same package)
                    if unit_pkg_ids[dep.index] == *pkg_id {
                        continue;
                    }
                    if dep_unit.mode == "run-custom-build" {
                        continue;
                    }
                    let key = (unit_pkg_ids[dep.index], &dep.extern_crate_name);
                    if seen.insert(key) {
                        deps.push(NixDep {
                            package_id: unit_pkg_ids[dep.index].to_string(),
                            extern_crate_name: dep.extern_crate_name.clone(),
                        });
                    }
                }
            }
            deps
        };

        // Build dependencies: from custom-build unit
        let build_dependencies: Vec<NixDep> = build_script_unit
            .map(|(_, bs_unit)| {
                bs_unit
                    .dependencies
                    .iter()
                    .map(|dep| NixDep {
                        package_id: unit_pkg_ids[dep.index].to_string(),
                        extern_crate_name: dep.extern_crate_name.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        // SHA256 from Cargo.lock
        let sha256 = checksums
            .get(&(crate_name.as_str(), version.as_str()))
            .map(|s| s.to_string());

        // Source info: prefer metadata, fall back to inferring from pkg_id.
        // Some crates appear in the unit graph but not in cargo metadata
        // (e.g., transitive deps pulled in by feature-specific resolution).
        let source = meta_pkg
            .and_then(|m| {
                parse_source(
                    m.source.as_deref(),
                    &m.manifest_path,
                    &metadata.workspace_root,
                )
            })
            .or_else(|| infer_source_from_pkg_id(pkg_id));

        // Crate root directory (from manifest_path, strip /Cargo.toml)
        let crate_root = meta_pkg
            .map(|m| {
                m.manifest_path
                    .rsplit_once("/Cargo.toml")
                    .map(|(dir, _)| dir)
                    .unwrap_or(&m.manifest_path)
            })
            .unwrap_or("");

        // Make an absolute src_path relative to the crate root
        let make_relative = |abs_path: &str| -> String {
            abs_path
                .strip_prefix(crate_root)
                .and_then(|s| s.strip_prefix('/'))
                .unwrap_or(abs_path)
                .to_string()
        };

        // Lib info
        let lib_path = if let Some((_, lu)) = lib_unit {
            let rel = make_relative(&lu.target.src_path);
            if rel == "src/lib.rs" {
                None
            } else {
                Some(rel)
            }
        } else {
            None
        };

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

        // Binary targets (only for workspace members / roots)
        let is_workspace_member = metadata
            .workspace_members
            .iter()
            .any(|wm| wm.starts_with(pkg_id));
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

        // Package metadata for CARGO_PKG_* env vars
        let authors = meta_pkg
            .and_then(|m| m.authors.clone())
            .unwrap_or_default();
        let description = meta_pkg.and_then(|m| m.description.clone());
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

    // Roots
    let roots: Vec<String> = unit_graph
        .roots
        .iter()
        .map(|&idx| unit_graph.units[idx].pkg_id.clone())
        .collect();

    // Workspace members: map name → package ID for members present in the build plan.
    // Only includes members that actually appear in the resolved dependency graph,
    // not all workspace members (some may be excluded by feature/package selection).
    let workspace_members: BTreeMap<String, String> = metadata
        .workspace_members
        .iter()
        .filter_map(|wm_id| {
            // workspace_member IDs match the pkg_id format used as keys in crates
            if crates.contains_key(wm_id) {
                let name = crates[wm_id].crate_name.clone();
                Some((name, wm_id.clone()))
            } else {
                None
            }
        })
        .collect();

    // Validate: every dependency reference must resolve to a crate in the plan
    let mut missing_refs: Vec<(String, String)> = Vec::new();
    for (pkg_id, crate_info) in &crates {
        for dep in &crate_info.dependencies {
            if !crates.contains_key(&dep.package_id) {
                missing_refs.push((pkg_id.clone(), dep.package_id.clone()));
            }
        }
        for dep in &crate_info.build_dependencies {
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
            "{} dependencies reference crates not in the build plan (likely a missing crate kind — see unit2nix bug tracker)",
            missing_refs.len()
        );
    }

    Ok(NixBuildPlan {
        version: 1,
        workspace_root: metadata.workspace_root.clone(),
        roots,
        workspace_members,
        target: target.map(|s| s.to_string()),
        cargo_lock_hash,
        crates,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_pkg_id;

    #[test]
    fn parse_registry_pkg_id() {
        let (name, version) =
            parse_pkg_id("registry+https://github.com/rust-lang/crates.io-index#serde@1.0.200");
        assert_eq!(name, "serde");
        assert_eq!(version, "1.0.200");
    }

    #[test]
    fn parse_path_pkg_id() {
        let (name, version) =
            parse_pkg_id("path+file:///home/user/project/crates/aspen-core#0.1.0");
        assert_eq!(name, "aspen-core");
        assert_eq!(version, "0.1.0");
    }

    #[test]
    fn parse_git_pkg_id() {
        let (name, version) =
            parse_pkg_id("git+https://github.com/example/repo.git?rev=abc123#my-crate@0.5.0");
        assert_eq!(name, "my-crate");
        assert_eq!(version, "0.5.0");
    }
}
