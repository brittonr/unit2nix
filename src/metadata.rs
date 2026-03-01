use serde::Deserialize;

// ---------------------------------------------------------------------------
// Cargo metadata types (from `cargo metadata --format-version=1`)
// ---------------------------------------------------------------------------

/// Deserialized output of `cargo metadata --format-version=1`.
#[derive(Debug, Deserialize)]
pub struct CargoMetadata {
    pub packages: Vec<MetadataPackage>,
    pub workspace_root: String,
    pub workspace_members: Vec<String>,
}

/// A package entry from cargo metadata.
///
/// Contains source info, manifest path, and optional metadata fields
/// used for `CARGO_PKG_*` environment variables in build scripts.
#[derive(Debug, Deserialize)]
pub struct MetadataPackage {
    pub id: String,
    pub source: Option<String>,
    pub links: Option<String>,
    pub manifest_path: String,
    pub authors: Option<Vec<String>>,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
}

// ---------------------------------------------------------------------------
// Cargo.lock types
// ---------------------------------------------------------------------------

/// Deserialized Cargo.lock file (TOML format).
#[derive(Debug, Deserialize)]
pub struct CargoLock {
    pub package: Option<Vec<LockPackage>>,
}

/// A package entry from Cargo.lock, providing checksums for registry crates.
#[derive(Debug, Deserialize)]
pub struct LockPackage {
    pub name: String,
    pub version: String,
    pub checksum: Option<String>,
}
