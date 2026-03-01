use std::process::Command;

use anyhow::{bail, Context, Result};
use sha2::{Sha256, Digest};

use crate::cli::Cli;
use crate::unit_graph::UnitGraph;
use crate::metadata::{CargoMetadata, CargoLock};

/// Run a cargo subcommand and return its stdout on success.
///
/// Creates a `Command::new("cargo")` with the given args plus
/// `--manifest-path`, runs it, checks the exit status, and returns
/// stdout bytes on success or bails with stderr on failure.
pub fn run_cargo(args: &[&str], manifest_path: &str, description: &str) -> Result<Vec<u8>> {
    let mut cmd = Command::new("cargo");
    cmd.args(args);
    cmd.args(["--manifest-path", manifest_path]);

    let output = cmd.output()
        .with_context(|| format!("failed to run {}", description))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{} failed:\n{}", description, stderr);
    }

    Ok(output.stdout)
}

/// Run `cargo build --unit-graph` and parse the result.
pub fn run_unit_graph(cli: &Cli) -> Result<UnitGraph> {
    let mut args = vec![
        "build",
        "--unit-graph",
        "-Z",
        "unstable-options",
        "--locked",
    ];

    // Build args vec from CLI options
    let features_str;
    let bin_str;
    let package_str;
    let target_str;

    if let Some(features) = &cli.features {
        features_str = features.clone();
        args.push("--features");
        args.push(&features_str);
    }
    if cli.all_features {
        args.push("--all-features");
    }
    if cli.no_default_features {
        args.push("--no-default-features");
    }
    if let Some(bin) = &cli.bin {
        bin_str = bin.clone();
        args.push("--bin");
        args.push(&bin_str);
    }
    if let Some(package) = &cli.package {
        package_str = package.clone();
        args.push("--package");
        args.push(&package_str);
    }
    if let Some(target) = &cli.target {
        target_str = target.clone();
        args.push("--target");
        args.push(&target_str);
    }

    let stdout = run_cargo(&args, &cli.manifest_path, "cargo build --unit-graph")?;
    serde_json::from_slice(&stdout).context("failed to parse unit graph JSON")
}

/// Run `cargo metadata` and parse the result.
pub fn run_cargo_metadata(cli: &Cli) -> Result<CargoMetadata> {
    let args = ["metadata", "--format-version=1", "--locked"];
    let stdout = run_cargo(&args, &cli.manifest_path, "cargo metadata")?;
    serde_json::from_slice(&stdout).context("failed to parse cargo metadata JSON")
}

/// Read and parse the Cargo.lock file.
pub fn read_cargo_lock(manifest_path: &str) -> Result<CargoLock> {
    let manifest = std::path::Path::new(manifest_path);
    let lock_path = manifest
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("Cargo.lock");

    let content = std::fs::read_to_string(&lock_path)
        .with_context(|| format!("failed to read {}", lock_path.display()))?;

    toml::from_str(&content).context("failed to parse Cargo.lock")
}

/// Compute SHA256 hash of the Cargo.lock file content.
///
/// Returns a hex-encoded hash string. The Nix consumer compares this
/// against `builtins.hashFile "sha256"` of the workspace's Cargo.lock
/// to detect stale build plans.
pub fn hash_cargo_lock(manifest_path: &str) -> Result<String> {
    let manifest = std::path::Path::new(manifest_path);
    let lock_path = manifest
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("Cargo.lock");

    let content = std::fs::read(&lock_path)
        .with_context(|| format!("failed to read {} for hashing", lock_path.display()))?;

    let hash = Sha256::digest(&content);
    Ok(format!("{:x}", hash))
}
