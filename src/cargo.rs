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
        .unwrap_or_else(|| Path::new("."))
        .join("Cargo.lock")
}

/// Run a cargo subcommand and return its stdout on success.
///
/// Creates a `Command::new("cargo")` with the given args plus
/// `--manifest-path`, runs it, checks the exit status, and returns
/// stdout bytes on success or bails with stderr on failure.
///
/// Respects the `CARGO` environment variable if set (standard Cargo convention).
///
/// # Errors
/// Returns an error if the cargo command fails to execute or exits with
/// a non-zero status code.
pub fn run_cargo(args: &[&str], manifest_path: &Path, description: &str) -> Result<Vec<u8>> {
    let cargo_bin = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut cmd = Command::new(&cargo_bin);
    cmd.args(args);
    cmd.arg("--manifest-path").arg(manifest_path);

    let output = cmd.output()
        .with_context(|| format!("failed to run {description}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let hint = if args.contains(&"--unit-graph") {
            "\n\nhint: `cargo build --unit-graph` requires nightly Rust \
             (`-Z unstable-options`). Is nightly installed and active?"
        } else {
            ""
        };
        if stdout.is_empty() {
            bail!("{description} failed:\n{stderr}{hint}");
        }
        let end = stdout
            .char_indices()
            .nth(500)
            .map_or(stdout.len(), |(i, _)| i);
        let preview = &stdout[..end];
        bail!("{description} failed:\nstderr: {stderr}\nstdout (truncated): {preview}{hint}");
    }

    Ok(output.stdout)
}

/// Append common CLI flags (features, target, bin, package, workspace) to an args vector.
fn append_common_args<'a>(args: &mut Vec<&'a str>, cli: &'a Cli) {
    if cli.workspace {
        args.push("--workspace");
    }
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
}

/// Run `cargo build --unit-graph` and parse the result.
///
/// # Errors
/// Returns an error if cargo fails or the output is not valid unit graph JSON.
pub fn run_unit_graph(cli: &Cli) -> Result<UnitGraph> {
    let mut args: Vec<&str> = vec![
        "build",
        "--unit-graph",
        "-Z",
        "unstable-options",
    ];
    if !cli.no_locked {
        args.push("--locked");
    }
    append_common_args(&mut args, cli);

    let stdout = run_cargo(&args, &cli.manifest_path, "cargo build --unit-graph")?;
    serde_json::from_slice(&stdout).context("failed to parse unit graph JSON")
}

/// Run `cargo test --unit-graph` and parse the result.
///
/// Like `run_unit_graph` but uses `test` instead of `build`, which includes
/// dev-dependencies and test targets in the unit graph.
///
/// # Errors
/// Returns an error if cargo fails or the output is not valid unit graph JSON.
pub fn run_test_unit_graph(cli: &Cli) -> Result<UnitGraph> {
    let mut args: Vec<&str> = vec![
        "test",
        "--unit-graph",
        "-Z",
        "unstable-options",
        "--no-run",
    ];
    if !cli.no_locked {
        args.push("--locked");
    }
    append_common_args(&mut args, cli);

    let stdout = run_cargo(&args, &cli.manifest_path, "cargo test --unit-graph")?;
    serde_json::from_slice(&stdout).context("failed to parse test unit graph JSON")
}

/// Run `cargo metadata` and parse the result.
///
/// # Errors
/// Returns an error if cargo fails or the output is not valid metadata JSON.
pub fn run_cargo_metadata(cli: &Cli) -> Result<CargoMetadata> {
    let mut args = vec!["metadata", "--format-version=1"];
    if !cli.no_locked {
        args.push("--locked");
    }
    let args = args;
    let stdout = run_cargo(&args, &cli.manifest_path, "cargo metadata")?;
    serde_json::from_slice(&stdout).context("failed to parse cargo metadata JSON")
}

/// Read and parse the Cargo.lock file, returning both the parsed lock
/// and its SHA256 hash.
///
/// Reads the file once to avoid redundant I/O.  The Nix consumer compares
/// the hash against `builtins.hashFile "sha256"` of the workspace's
/// `Cargo.lock` to detect stale build plans.
///
/// # Errors
/// Returns an error if `Cargo.lock` is missing, unreadable, or not valid TOML.
pub fn read_cargo_lock(manifest_path: &Path) -> Result<(CargoLock, String)> {
    let lock_path = cargo_lock_path(manifest_path);
    let content = std::fs::read(&lock_path)
        .with_context(|| {
            format!(
                "failed to read {}. Run `cargo generate-lockfile` or `cargo update` first",
                lock_path.display()
            )
        })?;
    let hash = Sha256::digest(&content);
    let text = String::from_utf8(content)
        .context("Cargo.lock is not valid UTF-8")?;
    let lock: CargoLock = toml::from_str(&text)
        .context("failed to parse Cargo.lock")?;
    Ok((lock, format!("{hash:x}")))
}

#[cfg(test)]
mod tests {
    use super::read_cargo_lock;
    use std::path::Path;

    #[test]
    fn cargo_lock_hash_is_sha256_hex() {
        // Hash our own Cargo.lock as a smoke test
        let (_lock, hash) = read_cargo_lock(Path::new("./Cargo.toml"))
            .expect("should read Cargo.lock");
        assert_eq!(hash.len(), 64, "SHA256 hex should be 64 chars, got: {hash}");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hash should be hex, got: {hash}"
        );
    }

    #[test]
    fn cargo_lock_hash_is_deterministic() {
        let (_, h1) = read_cargo_lock(Path::new("./Cargo.toml")).unwrap();
        let (_, h2) = read_cargo_lock(Path::new("./Cargo.toml")).unwrap();
        assert_eq!(h1, h2, "same file should produce same hash");
    }

    #[test]
    fn cargo_lock_parses_and_hashes_together() {
        let (lock, hash) = read_cargo_lock(Path::new("./Cargo.toml")).unwrap();
        assert!(!hash.is_empty(), "hash should not be empty");
        // Our own Cargo.lock should have packages
        assert!(
            lock.package.is_some(),
            "Cargo.lock should have a [[package]] section"
        );
    }
}
