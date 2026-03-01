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

    /// Enable all features
    #[arg(long)]
    all_features: bool,

    /// Do not activate the `default` feature
    #[arg(long)]
    no_default_features: bool,

    /// Target triple for cross-compilation (e.g. aarch64-unknown-linux-gnu)
    #[arg(long)]
    target: Option<String>,

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
#[allow(dead_code)]
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
#[allow(dead_code)]
struct MetadataPackage {
    id: String,
    name: String,
    version: String,
    source: Option<String>,
    targets: Vec<MetadataTarget>,
    links: Option<String>,
    manifest_path: String,
    authors: Option<Vec<String>>,
    description: Option<String>,
    homepage: Option<String>,
    license: Option<String>,
    repository: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
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
#[allow(dead_code)]
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
    /// Workspace member name → package ID (from cargo metadata).
    workspace_members: BTreeMap<String, String>,
    /// Target triple this plan was resolved for (null = host).
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<String>,
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
    build: Option<String>,
    lib_path: Option<String>,
    lib_name: Option<String>,
    lib_crate_types: Vec<String>,
    crate_bin: Vec<NixBinTarget>,
    links: Option<String>,
    // Package metadata (for CARGO_PKG_* env vars in build scripts)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    authors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repository: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum NixSource {
    CratesIo,
    /// Non-crates.io registry (e.g. corporate Artifactory).
    Registry {
        /// Registry index URL.
        index: String,
    },
    Local { path: String },
    Git {
        url: String,
        rev: String,
        /// Subdirectory within the git repo (for monorepo deps).
        /// Only present when the crate isn't at the repo root.
        #[serde(skip_serializing_if = "Option::is_none")]
        sub_dir: Option<String>,
        /// SHA256 hash from nix-prefetch-git. When present, the Nix consumer
        /// uses `pkgs.fetchgit` (a fixed-output derivation) for pure evaluation.
        /// When absent, falls back to `builtins.fetchGit` (requires --impure).
        #[serde(skip_serializing_if = "Option::is_none")]
        sha256: Option<String>,
    },
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
    cmd.args(["build", "--unit-graph", "-Z", "unstable-options", "--locked"]);
    cmd.args(["--manifest-path", &cli.manifest_path]);

    if let Some(features) = &cli.features {
        cmd.args(["--features", features]);
    }
    if cli.all_features {
        cmd.arg("--all-features");
    }
    if cli.no_default_features {
        cmd.arg("--no-default-features");
    }
    if let Some(bin) = &cli.bin {
        cmd.args(["--bin", bin]);
    }
    if let Some(package) = &cli.package {
        cmd.args(["--package", package]);
    }
    if let Some(target) = &cli.target {
        cmd.args(["--target", target]);
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
    cmd.args(["metadata", "--format-version=1", "--locked"]);
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
///
/// For git dependencies, computes `sub_dir` from the manifest_path relative
/// to Cargo's git checkout cache. This handles monorepo git deps where the
/// crate lives in a subdirectory (e.g., `{ git = "...", subdirectory = "crates/foo" }`).
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
        Some(s) if s.starts_with("registry+") => {
            // Extract registry URL for non-crates.io registries
            let registry_url = s.strip_prefix("registry+").unwrap_or("");
            if registry_url == "https://github.com/rust-lang/crates.io-index" {
                Some(NixSource::CratesIo)
            } else {
                Some(NixSource::Registry {
                    index: registry_url.to_string(),
                })
            }
        }
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

            // Compute subdirectory from manifest_path.
            // Cargo's git checkout lives at ~/.cargo/git/checkouts/<repo>/<hash>/
            // The manifest_path is e.g. ~/.cargo/git/checkouts/repo/abc123/crates/foo/Cargo.toml
            // We want sub_dir = "crates/foo"
            let sub_dir = compute_git_subdir(manifest_path);

            Some(NixSource::Git { url, rev, sub_dir, sha256: None })
        }
        _ => None,
    }
}

/// Compute the subdirectory of a crate within a git checkout.
///
/// Cargo stores git checkouts at `~/.cargo/git/checkouts/<name>/<hash>/`.
/// If the crate's Cargo.toml is at `.../checkouts/<name>/<hash>/sub/path/Cargo.toml`,
/// this returns `Some("sub/path")`. Returns `None` if the crate is at the repo root.
fn compute_git_subdir(manifest_path: &str) -> Option<String> {
    // Look for the checkouts/<name>/<hash>/ pattern
    let parts: Vec<&str> = manifest_path.split('/').collect();
    let checkout_idx = parts.iter().position(|&p| p == "checkouts")?;

    // checkouts/<name>/<hash>/ is 3 components after "checkouts"
    let root_idx = checkout_idx + 3;
    if root_idx >= parts.len() {
        return None;
    }

    // Everything between the checkout root and "Cargo.toml" is the subdirectory
    let end_idx = parts.len() - 1; // skip "Cargo.toml"
    if parts[end_idx] != "Cargo.toml" {
        return None;
    }

    if end_idx <= root_idx {
        return None; // Crate is at repo root
    }

    let sub = parts[root_idx..end_idx].join("/");
    if sub.is_empty() { None } else { Some(sub) }
}

/// Infer source type from a package ID when metadata is missing.
///
/// Some crates appear in the unit graph but not in `cargo metadata` (e.g.,
/// transitive deps pulled in by feature-specific resolution that metadata's
/// resolver doesn't include). The pkg_id format encodes the source:
/// - `registry+https://...#name@version` → crates.io or alternative registry
/// - `path+file:///...#version` → local
/// - `git+https://...#hash` → git
fn infer_source_from_pkg_id(pkg_id: &str) -> Option<NixSource> {
    if pkg_id.starts_with("registry+https://github.com/rust-lang/crates.io-index") {
        Some(NixSource::CratesIo)
    } else if pkg_id.starts_with("registry+") {
        let registry_url = pkg_id
            .strip_prefix("registry+")?
            .split('#')
            .next()?;
        Some(NixSource::Registry {
            index: registry_url.to_string(),
        })
    } else if pkg_id.starts_with("git+") {
        let without_prefix = pkg_id.strip_prefix("git+")?;
        let (url_part, rev) = without_prefix.rsplit_once('#')?;
        let url = url_part.split('?').next()?.to_string();
        Some(NixSource::Git {
            url,
            rev: rev.to_string(),
            sub_dir: None,
            sha256: None,
        })
    } else {
        // path+ or unknown — local
        None
    }
}

// ---------------------------------------------------------------------------
// Git prefetching
// ---------------------------------------------------------------------------

/// Prefetch result from `nix-prefetch-git`.
#[derive(Debug, Deserialize)]
struct PrefetchGitResult {
    sha256: String,
}

/// Run `nix-prefetch-git` to get the SHA256 of a git checkout.
///
/// This produces a fixed-output hash that `pkgs.fetchgit` can use,
/// enabling pure flake evaluation without `--impure`.
fn prefetch_git(url: &str, rev: &str) -> Result<String> {
    let output = Command::new("nix-prefetch-git")
        .args(["--url", url, "--rev", rev, "--fetch-submodules", "--quiet"])
        .output()
        .context("failed to run nix-prefetch-git (is it installed?)")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("nix-prefetch-git failed for {url} at {rev}:\n{stderr}");
    }

    let result: PrefetchGitResult = serde_json::from_slice(&output.stdout)
        .context("failed to parse nix-prefetch-git JSON output")?;

    Ok(result.sha256)
}

/// Prefetch all git sources in the build plan, filling in their sha256 fields.
///
/// Deduplicates by (url, rev) so each git repo is fetched at most once,
/// even if multiple crates come from the same repo (monorepo deps).
fn prefetch_git_sources(plan: &mut NixBuildPlan) -> Result<()> {
    // Collect unique (url, rev) pairs that need prefetching
    let mut to_prefetch: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for (pkg_id, crate_info) in &plan.crates {
        if let Some(NixSource::Git { url, rev, sha256: None, .. }) = &crate_info.source {
            to_prefetch
                .entry((url.clone(), rev.clone()))
                .or_default()
                .push(pkg_id.clone());
        }
    }

    if to_prefetch.is_empty() {
        return Ok(());
    }

    let total = to_prefetch.len();
    eprintln!("Prefetching {total} git source(s)...");

    for (idx, ((url, rev), pkg_ids)) in to_prefetch.iter().enumerate() {
        let short_rev = if rev.len() > 12 { &rev[..12] } else { rev };
        eprintln!("  [{}/{}] {} @ {}", idx + 1, total, url, short_rev);

        let sha256 = prefetch_git(url, rev)?;

        // Apply the hash to all crates from this repo
        for pkg_id in pkg_ids {
            if let Some(NixSource::Git { sha256: ref mut hash, .. }) = &mut plan.crates.get_mut(pkg_id).unwrap().source {
                *hash = Some(sha256.clone());
            }
        }
    }

    Ok(())
}

/// Returns true if the target kind represents a library (lib, rlib, cdylib, etc).
fn is_lib_kind(kind: &[String]) -> bool {
    kind.iter()
        .any(|k| matches!(k.as_str(), "lib" | "rlib" | "cdylib" | "dylib" | "staticlib"))
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

fn merge(unit_graph: &UnitGraph, metadata: &CargoMetadata, lock: &CargoLock, target: Option<&str>) -> Result<NixBuildPlan> {
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
        let lib_unit = units
            .iter()
            .find(|(_, u)| u.mode == "build" && is_lib_kind(&u.target.kind))
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

        // Features: union across all lib-like units for this package.
        // The same crate can appear multiple times in the unit graph with
        // different feature sets (e.g., hashbrown: once with no features for
        // a proc-macro's host dep, once with "default" for a target dep).
        // Nix builds one derivation per crate, so it needs the superset.
        let features = {
            let mut all_features: Vec<String> = Vec::new();
            for (_, u) in units {
                if u.mode == "build" && (is_lib_kind(&u.target.kind)
                    || u.target.kind.contains(&"proc-macro".to_string()))
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
        let proc_macro = primary.target.kind.contains(&"proc-macro".to_string());

        // Normal dependencies: union across the primary unit and all lib-like
        // units for this package. Different feature variants may pull in
        // different deps; bin-only crates have deps on the bin unit only.
        let dependencies: Vec<NixDep> = {
            let mut deps = Vec::new();
            let mut seen = std::collections::HashSet::new();
            let dep_units = units.iter().filter(|(_, u)| {
                u.mode == "build"
                    && (is_lib_kind(&u.target.kind)
                        || u.target.kind.contains(&"proc-macro".to_string())
                        || u.target.kind.contains(&"bin".to_string()))
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
    let mut plan = merge(&unit_graph, &metadata, &lock, cli.target.as_deref())?;
    eprintln!("  {} crates in build plan", plan.crates.len());
    eprintln!("  {} workspace members", plan.workspace_members.len());
    if let Some(ref t) = plan.target {
        eprintln!("  target: {t}");
    }

    // Prefetch git sources for pure flake evaluation
    prefetch_git_sources(&mut plan)?;

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
    fn parse_source_alternative_registry() {
        let source = parse_source(
            Some("registry+https://dl.cloudsmith.io/public/my-org/my-repo/cargo/index.git"),
            "",
            "",
        );
        match source {
            Some(NixSource::Registry { index }) => {
                assert_eq!(index, "https://dl.cloudsmith.io/public/my-org/my-repo/cargo/index.git");
            }
            other => panic!("expected Registry, got {other:?}"),
        }
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
            "/home/user/.cargo/git/checkouts/repo/abc123/Cargo.toml",
            "",
        );
        match source {
            Some(NixSource::Git { url, rev, sub_dir, sha256 }) => {
                assert_eq!(url, "https://github.com/example/repo.git");
                assert_eq!(rev, "abc123def456");
                assert_eq!(sub_dir, None, "root-level crate should have no sub_dir");
                assert_eq!(sha256, None, "sha256 is filled later by prefetch");
            }
            other => panic!("expected Git, got {other:?}"),
        }
    }

    #[test]
    fn parse_source_git_subdir() {
        let source = parse_source(
            Some("git+https://github.com/org/monorepo.git?rev=abc123#abc123def456"),
            "/home/user/.cargo/git/checkouts/monorepo/abc123/crates/my-crate/Cargo.toml",
            "",
        );
        match source {
            Some(NixSource::Git { url, rev, sub_dir, sha256 }) => {
                assert_eq!(url, "https://github.com/org/monorepo.git");
                assert_eq!(rev, "abc123def456");
                assert_eq!(sub_dir, Some("crates/my-crate".to_string()));
                assert_eq!(sha256, None, "sha256 is filled later by prefetch");
            }
            other => panic!("expected Git with sub_dir, got {other:?}"),
        }
    }

    #[test]
    fn compute_git_subdir_root() {
        assert_eq!(
            compute_git_subdir("/home/user/.cargo/git/checkouts/repo/abc123/Cargo.toml"),
            None
        );
    }

    #[test]
    fn compute_git_subdir_nested() {
        assert_eq!(
            compute_git_subdir("/home/user/.cargo/git/checkouts/repo/abc123/sub/path/Cargo.toml"),
            Some("sub/path".to_string())
        );
    }

    #[test]
    fn compute_git_subdir_no_checkouts() {
        assert_eq!(
            compute_git_subdir("/some/random/path/Cargo.toml"),
            None
        );
    }
}
