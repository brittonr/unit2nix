use std::path::PathBuf;

use clap::Parser;

/// Generate per-crate Nix build plans from Cargo's unit graph.
///
/// Merges `cargo build --unit-graph` (exact resolved features, deps, platform
/// filtering) with `cargo metadata` (source info, SHA256 hashes, links field)
/// into a single JSON consumed by a thin Nix wrapper around `buildRustCrate`.
#[derive(Parser, Debug)]
#[command(version, about)]
#[allow(clippy::struct_excessive_bools)]
pub struct Cli {
    /// Path to the Cargo.toml (default: ./Cargo.toml)
    #[arg(long, default_value = "./Cargo.toml")]
    pub manifest_path: PathBuf,

    /// Features to enable (comma-separated)
    #[arg(long)]
    pub features: Option<String>,

    /// Build a specific binary target
    #[arg(long)]
    pub bin: Option<String>,

    /// Build a specific package
    #[arg(short, long)]
    pub package: Option<String>,

    /// Enable all features
    #[arg(long)]
    pub all_features: bool,

    /// Do not activate the `default` feature
    #[arg(long)]
    pub no_default_features: bool,

    /// Target triple for cross-compilation (e.g. aarch64-unknown-linux-gnu)
    #[arg(long)]
    pub target: Option<String>,

    /// Output file (default: stdout)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Check which -sys crates need native overrides, then exit.
    /// Reads an existing build plan JSON (from -o) and reports coverage.
    #[arg(long)]
    pub check_overrides: bool,

    /// Include dev-dependencies for workspace members.
    ///
    /// Runs `cargo test --unit-graph` in addition to `cargo build --unit-graph`
    /// to capture dev-only dependencies. These are emitted as `devDependencies`
    /// on workspace members and used by the Nix consumer's `.test` output.
    #[arg(long)]
    pub include_dev: bool,
}
