use std::collections::BTreeMap;
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};

/// Generate per-crate Nix build plans from Cargo's unit graph.
///
/// Merges `cargo build --unit-graph` (exact resolved features, deps, platform
/// filtering) with `cargo metadata` (source info, SHA256 hashes, links field)
/// into a single JSON consumed by a thin Nix wrapper around `buildRustCrate`.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Cli {
    /// Path to the Cargo.toml (default: ./Cargo.toml)
    #[arg(long, default_value = "./Cargo.toml")]
    manifest_path: String,

    /// Features to enable (comma-separated)
    #[arg(long)]
    features: Option<String>,

    /// Build a specific binary target
    #[arg(long)]
    bin: Option<String>,

    /// Build a specific package
    #[arg(short, long)]
    package: Option<String>,

    /// Output file (default: stdout)
    #[arg(short, long)]
    output: Option<String>,
}

// ---------------------------------------------------------------------------
// Unit graph types (from `cargo build --unit-graph -Z unstable-options`)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct UnitGraph {
    units: Vec<Unit>,
    roots: Vec<usize>,
}

#[derive(Debug, Deserialize)]
struct Unit {
    pkg_id: String,
    target: UnitTarget,
    mode: String,
    features: Vec<String>,
    dependencies: Vec<UnitDep>,
}

#[derive(Debug, Deserialize)]
struct UnitTarget {
    kind: Vec<String>,
    crate_types: Vec<String>,
    name: String,
    src_path: String,
    edition: String,
}

#[derive(Debug, Deserialize)]
struct UnitDep {
    index: usize,
    extern_crate_name: String,
    public: bool,
}

// ---------------------------------------------------------------------------
// Cargo metadata types (from `cargo metadata --format-version=1`)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<MetadataPackage>,
    workspace_root: String,
    workspace_members: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct MetadataPackage {
    id: String,
    name: String,
    version: String,
    source: Option<String>,
    targets: Vec<MetadataTarget>,
    links: Option<String>,
    manifest_path: String,
}

#[derive(Debug, Deserialize)]
struct MetadataTarget {
    kind: Vec<String>,
    name: String,
    src_path: String,
}

// ---------------------------------------------------------------------------
// Cargo.lock types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct CargoLock {
    package: Option<Vec<LockPackage>>,
}

#[derive(Debug, Deserialize)]
struct LockPackage {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
}

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NixBuildPlan {
    version: u32,
    workspace_root: String,
    roots: Vec<String>,
    crates: BTreeMap<String, NixCrate>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NixCrate {
    crate_name: String,
    version: String,
    edition: String,
    sha256: Option<String>,
    source: Option<NixSource>,
    features: Vec<String>,
    dependencies: Vec<NixDep>,
    build_dependencies: Vec<NixDep>,
    proc_macro: bool,
    has_build_script: bool,
    lib_path: Option<String>,
    lib_name: Option<String>,
    lib_crate_types: Vec<String>,
    crate_bin: Vec<NixBinTarget>,
    links: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum NixSource {
    CratesIo,
    Local { path: String },
    Git { url: String, rev: String },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NixDep {
    package_id: String,
    extern_crate_name: String,
}

#[derive(Debug, Serialize)]
struct NixBinTarget {
    name: String,
    path: String,
}

// ---------------------------------------------------------------------------
// Core logic
// ---------------------------------------------------------------------------

fn run_unit_graph(cli: &Cli) -> Result<UnitGraph> {
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "--unit-graph", "-Z", "unstable-options"]);
    cmd.args(["--manifest-path", &cli.manifest_path]);

    if let Some(features) = &cli.features {
        cmd.args(["--features", features]);
    }
    if let Some(bin) = &cli.bin {
        cmd.args(["--bin", bin]);
    }
    if let Some(package) = &cli.package {
        cmd.args(["--package", package]);
    }

    let output = cmd.output().context("failed to run `cargo build --unit-graph`")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("cargo build --unit-graph failed:\n{stderr}");
    }

    serde_json::from_slice(&output.stdout).context("failed to parse unit graph JSON")
}

fn run_cargo_metadata(cli: &Cli) -> Result<CargoMetadata> {
    let mut cmd = Command::new("cargo");
    cmd.args(["metadata", "--format-version=1"]);
    cmd.args(["--manifest-path", &cli.manifest_path]);

    let output = cmd.output().context("failed to run `cargo metadata`")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("cargo metadata failed:\n{stderr}");
    }

    serde_json::from_slice(&output.stdout).context("failed to parse cargo metadata JSON")
}

fn read_cargo_lock(manifest_path: &str) -> Result<CargoLock> {
    let manifest = std::path::Path::new(manifest_path);
    let lock_path = manifest
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("Cargo.lock");

    let content = std::fs::read_to_string(&lock_path)
        .with_context(|| format!("failed to read {}", lock_path.display()))?;

    toml::from_str(&content).context("failed to parse Cargo.lock")
}

/// Parse source string from cargo metadata into a NixSource.
fn parse_source(source: Option<&str>, manifest_path: &str, workspace_root: &str) -> Option<NixSource> {
    match source {
        None => {
            // Local path dependency — compute relative path from workspace root
            let manifest = std::path::Path::new(manifest_path);
            let crate_dir = manifest.parent().unwrap_or(std::path::Path::new("."));
            let crate_dir_str = crate_dir.to_string_lossy().to_string();
            let ws = workspace_root.trim_end_matches('/');
            let rel = crate_dir_str
                .strip_prefix(ws)
                .map(|s| s.strip_prefix('/').unwrap_or(s))
                .unwrap_or(&crate_dir_str);
            let path = if rel.is_empty() { "." } else { rel };
            Some(NixSource::Local {
                path: path.to_string(),
            })
        }
        Some(s) if s.starts_with("registry+") => Some(NixSource::CratesIo),
        Some(s) if s.starts_with("git+") => {
            // Format: git+URL?rev=HASH#HASH or git+URL#HASH
            let without_prefix = s.strip_prefix("git+").unwrap_or(s);
            let (url_part, rev) = if let Some((url, hash)) = without_prefix.rsplit_once('#') {
                (url.to_string(), hash.to_string())
            } else {
                (without_prefix.to_string(), String::new())
            };
            // Strip query params from URL
            let url = url_part.split('?').next().unwrap_or(&url_part).to_string();
            Some(NixSource::Git { url, rev })
        }
        _ => None,
    }
}

/// Extract name@version from a pkg_id string.
/// Formats: "registry+...#name@version" or "path+file:///...#version"
fn parse_pkg_id(pkg_id: &str) -> (String, String) {
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

fn merge(unit_graph: &UnitGraph, metadata: &CargoMetadata, lock: &CargoLock) -> Result<NixBuildPlan> {
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
        // Find the primary lib unit (or first bin unit)
        let lib_unit = units
            .iter()
            .find(|(_, u)| u.mode == "build" && u.target.kind.contains(&"lib".to_string()))
            .or_else(|| {
                units
                    .iter()
                    .find(|(_, u)| u.mode == "build" && u.target.kind.contains(&"proc-macro".to_string()))
            });

        let bin_units: Vec<&(usize, &Unit)> = units
            .iter()
            .filter(|(_, u)| u.mode == "build" && u.target.kind.contains(&"bin".to_string()))
            .collect();

        let build_script_unit = units
            .iter()
            .find(|(_, u)| u.mode == "build" && u.target.kind.contains(&"custom-build".to_string()));

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

        // Features from the primary build unit
        let features = primary.features.clone();

        // Is this a proc-macro?
        let proc_macro = primary.target.kind.contains(&"proc-macro".to_string());

        // Normal dependencies: from lib/proc-macro unit, excluding self's build script
        let dependencies: Vec<NixDep> = primary
            .dependencies
            .iter()
            .filter(|dep| {
                let dep_unit = &unit_graph.units[dep.index];
                // Skip run-custom-build (self's build script)
                dep_unit.mode != "run-custom-build"
            })
            .map(|dep| NixDep {
                package_id: unit_pkg_ids[dep.index].to_string(),
                extern_crate_name: dep.extern_crate_name.clone(),
            })
            .collect();

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

        // Source info
        let source = meta_pkg.and_then(|m| {
            parse_source(
                m.source.as_deref(),
                &m.manifest_path,
                &metadata.workspace_root,
            )
        });

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
                has_build_script: build_script_unit.is_some(),
                lib_path,
                lib_name,
                lib_crate_types,
                crate_bin,
                links,
            },
        );
    }

    // Roots
    let roots: Vec<String> = unit_graph
        .roots
        .iter()
        .map(|&idx| unit_graph.units[idx].pkg_id.clone())
        .collect();

    Ok(NixBuildPlan {
        version: 1,
        workspace_root: metadata.workspace_root.clone(),
        roots,
        crates,
    })
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    eprintln!("Running cargo build --unit-graph...");
    let unit_graph = run_unit_graph(&cli)?;
    eprintln!("  {} units, {} roots", unit_graph.units.len(), unit_graph.roots.len());

    eprintln!("Running cargo metadata...");
    let metadata = run_cargo_metadata(&cli)?;
    eprintln!("  {} packages", metadata.packages.len());

    eprintln!("Reading Cargo.lock...");
    let lock = read_cargo_lock(&cli.manifest_path)?;
    eprintln!(
        "  {} packages with checksums",
        lock.package
            .as_ref()
            .map(|p| p.iter().filter(|p| p.checksum.is_some()).count())
            .unwrap_or(0)
    );

    eprintln!("Merging...");
    let plan = merge(&unit_graph, &metadata, &lock)?;
    eprintln!("  {} crates in build plan", plan.crates.len());

    let json = serde_json::to_string_pretty(&plan)?;

    match &cli.output {
        Some(path) => {
            std::fs::write(path, &json)?;
            eprintln!("Wrote {path}");
        }
        None => println!("{json}"),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn parse_source_crates_io() {
        let source = parse_source(
            Some("registry+https://github.com/rust-lang/crates.io-index"),
            "",
            "",
        );
        assert!(matches!(source, Some(NixSource::CratesIo)));
    }

    #[test]
    fn parse_source_local() {
        let source = parse_source(None, "/home/user/project/crates/foo/Cargo.toml", "/home/user/project");
        match source {
            Some(NixSource::Local { path }) => assert_eq!(path, "crates/foo"),
            other => panic!("expected Local, got {other:?}"),
        }
    }

    #[test]
    fn parse_source_local_root() {
        let source = parse_source(None, "/home/user/project/Cargo.toml", "/home/user/project");
        match source {
            Some(NixSource::Local { path }) => assert_eq!(path, "."),
            other => panic!("expected Local with '.', got {other:?}"),
        }
    }

    #[test]
    fn parse_source_git() {
        let source = parse_source(
            Some("git+https://github.com/example/repo.git?rev=abc123#abc123def456"),
            "",
            "",
        );
        match source {
            Some(NixSource::Git { url, rev }) => {
                assert_eq!(url, "https://github.com/example/repo.git");
                assert_eq!(rev, "abc123def456");
            }
            other => panic!("expected Git, got {other:?}"),
        }
    }
}
