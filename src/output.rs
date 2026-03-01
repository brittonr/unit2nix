use std::collections::BTreeMap;
use serde::Serialize;

/// Schema version for the build plan JSON output.
pub const BUILD_PLAN_VERSION: u32 = 1;

/// Top-level build plan consumed by `build-from-unit-graph.nix`.
///
/// Contains every crate in the dependency graph with its resolved features,
/// dependencies, source info, and metadata. The Nix consumer walks this
/// to build each crate individually with `buildRustCrate`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NixBuildPlan {
    pub version: u32,
    pub workspace_root: String,
    pub roots: Vec<String>,
    /// Workspace member name → package ID (from cargo metadata).
    pub workspace_members: BTreeMap<String, String>,
    /// Target triple this plan was resolved for (null = host).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// SHA256 hash of the Cargo.lock file content.
    /// Used by the Nix consumer to detect stale build plans.
    pub cargo_lock_hash: String,
    pub crates: BTreeMap<String, NixCrate>,
}

/// A single crate in the build plan, with everything needed to build it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NixCrate {
    pub crate_name: String,
    pub version: String,
    pub edition: String,
    pub sha256: Option<String>,
    pub source: Option<NixSource>,
    pub features: Vec<String>,
    pub dependencies: Vec<NixDep>,
    pub build_dependencies: Vec<NixDep>,
    pub proc_macro: bool,
    pub build: Option<String>,
    pub lib_path: Option<String>,
    pub lib_name: Option<String>,
    pub lib_crate_types: Vec<String>,
    pub crate_bin: Vec<NixBinTarget>,
    pub links: Option<String>,
    // Package metadata (for CARGO_PKG_* env vars in build scripts)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
}

/// Where a crate's source comes from.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum NixSource {
    /// The default crates.io registry.
    CratesIo,
    /// A non-crates.io registry (e.g. corporate Artifactory).
    Registry {
        /// Registry index URL.
        index: String,
    },
    /// A local path dependency within the workspace.
    Local { path: String },
    /// A git dependency.
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

/// A dependency reference in the build plan.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NixDep {
    pub package_id: String,
    pub extern_crate_name: String,
}

/// A binary target within a crate.
#[derive(Debug, Serialize)]
pub struct NixBinTarget {
    pub name: String,
    pub path: String,
}
