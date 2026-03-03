use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use sha2::{Sha256, Digest};

use crate::cli::Cli;
use crate::unit_graph::UnitGraph;
use crate::metadata::{CargoMetadata, CargoLock};

/// Resolve the Cargo.lock path from a manifest path.
fn cargo_lock_path(manifest_path: &Path) -> PathBuf {
    manifest_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("Cargo.lock")
}

/// Run a cargo subcommand and return its stdout on success.
///
/// Creates a `Command::new("cargo")` with the given args plus
/// `--manifest-path`, runs it, checks the exit status, and returns
/// stdout bytes on success or bails with stderr on failure.
pub fn run_cargo(args: &[&str], manifest_path: &Path, description: &str) -> Result<Vec<u8>> {
    let mut cmd = Command::new("cargo");
    cmd.args(args);
    cmd.arg("--manifest-path").arg(manifest_path);

    let output = cmd.output()
        .with_context(|| format!("failed to run {}", description))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.is_empty() {
            bail!("{} failed:\n{}", description, stderr);
        } else {
            let end = stdout.char_indices().nth(500).map_or(stdout.len(), |(i, _)| i);
            let preview = &stdout[..end];
            bail!("{} failed:\nstderr: {}\nstdout (truncated): {}", description, stderr, preview);
        }
    }

    Ok(output.stdout)
}

/// Run `cargo build --unit-graph` and parse the result.
pub fn run_unit_graph(cli: &Cli) -> Result<UnitGraph> {
    let mut args: Vec<&str> = vec![
        "build",
        "--unit-graph",
        "-Z",
        "unstable-options",
        "--locked",
    ];

    if let Some(features) = cli.features.as_deref() {
        args.extend_from_slice(&["--features", features]);
    }
    if cli.all_features {
        args.push("--all-features");
    }
    if cli.no_default_features {
        args.push("--no-default-features");
    }
    if let Some(bin) = cli.bin.as_deref() {
        args.extend_from_slice(&["--bin", bin]);
    }
    if let Some(package) = cli.package.as_deref() {
        args.extend_from_slice(&["--package", package]);
    }
    if let Some(target) = cli.target.as_deref() {
        args.extend_from_slice(&["--target", target]);
    }

    let stdout = run_cargo(&args, &cli.manifest_path, "cargo build --unit-graph")?;
    serde_json::from_slice(&stdout).context("failed to parse unit graph JSON")
}

/// Run `cargo test --unit-graph` and parse the result.
///
/// Like `run_unit_graph` but uses `test` instead of `build`, which includes
/// dev-dependencies and test targets in the unit graph.
pub fn run_test_unit_graph(cli: &Cli) -> Result<UnitGraph> {
    let mut args: Vec<&str> = vec![
        "test",
        "--unit-graph",
        "-Z",
        "unstable-options",
        "--locked",
        "--no-run",
    ];

    if let Some(features) = cli.features.as_deref() {
        args.extend_from_slice(&["--features", features]);
    }
    if cli.all_features {
        args.push("--all-features");
    }
    if cli.no_default_features {
        args.push("--no-default-features");
    }
    if let Some(bin) = cli.bin.as_deref() {
        args.extend_from_slice(&["--bin", bin]);
    }
    if let Some(package) = cli.package.as_deref() {
        args.extend_from_slice(&["--package", package]);
    }
    if let Some(target) = cli.target.as_deref() {
        args.extend_from_slice(&["--target", target]);
    }

    let stdout = run_cargo(&args, &cli.manifest_path, "cargo test --unit-graph")?;
    serde_json::from_slice(&stdout).context("failed to parse test unit graph JSON")
}

/// Run `cargo metadata` and parse the result.
pub fn run_cargo_metadata(cli: &Cli) -> Result<CargoMetadata> {
    let args = ["metadata", "--format-version=1", "--locked"];
    let stdout = run_cargo(&args, &cli.manifest_path, "cargo metadata")?;
    serde_json::from_slice(&stdout).context("failed to parse cargo metadata JSON")
}

/// Read and parse the Cargo.lock file.
pub fn read_cargo_lock(manifest_path: &Path) -> Result<CargoLock> {
    let lock_path = cargo_lock_path(manifest_path);
    let content = std::fs::read_to_string(&lock_path)
        .with_context(|| format!("failed to read {}", lock_path.display()))?;
    toml::from_str(&content).context("failed to parse Cargo.lock")
}

/// Compute SHA256 hash of the Cargo.lock file content.
///
/// Returns a hex-encoded hash string. The Nix consumer compares this
/// against `builtins.hashFile "sha256"` of the workspace's Cargo.lock
/// to detect stale build plans.
pub fn hash_cargo_lock(manifest_path: &Path) -> Result<String> {
    let lock_path = cargo_lock_path(manifest_path);
    let content = std::fs::read(&lock_path)
        .with_context(|| format!("failed to read {} for hashing", lock_path.display()))?;
    let hash = Sha256::digest(&content);
    Ok(format!("{:x}", hash))
}

#[cfg(test)]
mod tests {
    use super::hash_cargo_lock;
    use std::path::Path;

    #[test]
    fn cargo_lock_hash_is_sha256_hex() {
        // Hash our own Cargo.lock as a smoke test
        let hash = hash_cargo_lock(Path::new("./Cargo.toml")).expect("should hash Cargo.lock");
        assert_eq!(hash.len(), 64, "SHA256 hex should be 64 chars, got: {hash}");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hash should be hex, got: {hash}"
        );
    }

    #[test]
    fn cargo_lock_hash_is_deterministic() {
        let h1 = hash_cargo_lock(Path::new("./Cargo.toml")).unwrap();
        let h2 = hash_cargo_lock(Path::new("./Cargo.toml")).unwrap();
        assert_eq!(h1, h2, "same file should produce same hash");
    }
}
