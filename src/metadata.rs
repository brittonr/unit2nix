use serde::Deserialize;

// ---------------------------------------------------------------------------
// Cargo metadata types (from `cargo metadata --format-version=1`)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CargoMetadata {
    pub packages: Vec<MetadataPackage>,
    pub workspace_root: String,
    pub workspace_members: Vec<String>,
}

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

#[derive(Debug, Deserialize)]
pub struct CargoLock {
    pub package: Option<Vec<LockPackage>>,
}

#[derive(Debug, Deserialize)]
pub struct LockPackage {
    pub name: String,
    pub version: String,
    pub checksum: Option<String>,
}
