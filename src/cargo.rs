use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::cli::Cli;
use crate::metadata::{CargoLock, CargoMetadata};
use crate::unit_graph::UnitGraph;

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

    let output = cmd
        .output()
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
fn append_common_args(args: &mut Vec<String>, cli: &Cli) {
    if cli.workspace {
        args.push("--workspace".to_string());
    }
    if let Some(features) = cli.features.as_deref() {
        args.push("--features".to_string());
        args.push(features.to_string());
    }
    if cli.all_features {
        args.push("--all-features".to_string());
    }
    if cli.no_default_features {
        args.push("--no-default-features".to_string());
    }
    if let Some(bin) = cli.bin.as_deref() {
        args.push("--bin".to_string());
        args.push(bin.to_string());
    }
    if let Some(package) = cli.package.as_deref() {
        args.push("--package".to_string());
        args.push(package.to_string());
    }
    if let Some(target) = cli.target.as_deref() {
        args.push("--target".to_string());
        args.push(target.to_string());
    }
    if let Some(build_std) = cli.build_std.as_deref() {
        args.push(format!("-Zbuild-std={build_std}"));
    }
    if let Some(features) = cli.build_std_features.as_deref() {
        args.push(format!("-Zbuild-std-features={features}"));
    }
}

fn unit_graph_args(cli: &Cli) -> Vec<String> {
    let mut args = vec![
        "build".to_string(),
        "--unit-graph".to_string(),
        "-Z".to_string(),
        "unstable-options".to_string(),
    ];
    if !cli.no_locked {
        args.push("--locked".to_string());
    }
    append_common_args(&mut args, cli);
    args
}

fn test_unit_graph_args(cli: &Cli) -> Vec<String> {
    let mut args = vec![
        "test".to_string(),
        "--unit-graph".to_string(),
        "-Z".to_string(),
        "unstable-options".to_string(),
        "--no-run".to_string(),
    ];
    if !cli.no_locked {
        args.push("--locked".to_string());
    }
    append_common_args(&mut args, cli);
    args
}

fn cargo_metadata_args(cli: &Cli) -> Vec<String> {
    let mut args = vec!["metadata".to_string(), "--format-version=1".to_string()];
    if !cli.no_locked {
        args.push("--locked".to_string());
    }
    if let Some(features) = cli.features.as_deref() {
        args.push("--features".to_string());
        args.push(features.to_string());
    }
    if cli.all_features {
        args.push("--all-features".to_string());
    }
    if cli.no_default_features {
        args.push("--no-default-features".to_string());
    }
    args
}

/// Run `cargo build --unit-graph` and parse the result.
///
/// # Errors
/// Returns an error if cargo fails or the output is not valid unit graph JSON.
pub fn run_unit_graph(cli: &Cli) -> Result<UnitGraph> {
    let args = unit_graph_args(cli);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

    let stdout = run_cargo(&arg_refs, &cli.manifest_path, "cargo build --unit-graph")?;
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
    let args = test_unit_graph_args(cli);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

    let stdout = run_cargo(&arg_refs, &cli.manifest_path, "cargo test --unit-graph")?;
    serde_json::from_slice(&stdout).context("failed to parse test unit graph JSON")
}

/// Run `cargo metadata` and parse the result.
///
/// Only feature flags are forwarded — `cargo metadata` doesn't accept
/// `--workspace`, `--package`, `--bin`, `--target`, or `-Z` flags.
/// It doesn't need them: metadata always resolves the full workspace
/// with all members. Feature flags matter because they affect which
/// optional dependencies get resolved (and thus appear in `packages[]`).
///
/// # Errors
/// Returns an error if cargo fails or the output is not valid metadata JSON.
pub fn run_cargo_metadata(cli: &Cli) -> Result<CargoMetadata> {
    let args = cargo_metadata_args(cli);
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let stdout = run_cargo(&arg_refs, &cli.manifest_path, "cargo metadata")?;
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
    let content = std::fs::read(&lock_path).with_context(|| {
        format!(
            "failed to read {}. Run `cargo generate-lockfile` or `cargo update` first",
            lock_path.display()
        )
    })?;
    let hash = Sha256::digest(&content);
    let text = String::from_utf8(content).context("Cargo.lock is not valid UTF-8")?;
    let lock: CargoLock = toml::from_str(&text).context("failed to parse Cargo.lock")?;
    Ok((lock, format!("{hash:x}")))
}

#[cfg(test)]
mod tests {
    use super::{
        cargo_metadata_args, read_cargo_lock, run_cargo, test_unit_graph_args, unit_graph_args,
    };
    use crate::cli::Cli;
    use crate::test_support::env_lock;
    use std::path::{Path, PathBuf};

    fn write_fake_cargo(script_body: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fake-cargo");
        std::fs::write(
            &path,
            format!("#!/usr/bin/env bash\nset -euo pipefail\n{script_body}\n"),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }
        dir
    }

    fn with_fake_cargo<T>(script_body: &str, f: impl FnOnce() -> T) -> T {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let dir = write_fake_cargo(script_body);
        let path = dir.path().join("fake-cargo");
        let old = std::env::var_os("CARGO");
        std::env::set_var("CARGO", &path);
        let result = f();
        match old {
            Some(value) => std::env::set_var("CARGO", value),
            None => std::env::remove_var("CARGO"),
        }
        result
    }

    fn make_cli() -> Cli {
        Cli {
            manifest_path: PathBuf::from("./Cargo.toml"),
            features: None,
            bin: None,
            package: None,
            all_features: false,
            no_default_features: false,
            target: None,
            output: PathBuf::from("build-plan.json"),
            stdout: false,
            check_overrides: false,
            include_dev: false,
            members: None,
            no_check: false,
            json: false,
            force: false,
            workspace: false,
            no_locked: false,
            build_std: None,
            build_std_features: None,
        }
    }

    #[test]
    fn unit_graph_args_include_common_flags() {
        let mut cli = make_cli();
        cli.workspace = true;
        cli.features = Some("serde,cli".to_string());
        cli.all_features = true;
        cli.no_default_features = true;
        cli.bin = Some("unit2nix".to_string());
        cli.package = Some("unit2nix".to_string());
        cli.target = Some("aarch64-unknown-linux-gnu".to_string());
        cli.build_std = Some("core,alloc".to_string());
        cli.build_std_features = Some("compiler-builtins-mem".to_string());

        assert_eq!(
            unit_graph_args(&cli),
            vec![
                "build",
                "--unit-graph",
                "-Z",
                "unstable-options",
                "--locked",
                "--workspace",
                "--features",
                "serde,cli",
                "--all-features",
                "--no-default-features",
                "--bin",
                "unit2nix",
                "--package",
                "unit2nix",
                "--target",
                "aarch64-unknown-linux-gnu",
                "-Zbuild-std=core,alloc",
                "-Zbuild-std-features=compiler-builtins-mem",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn unit_graph_args_skip_locked_when_requested() {
        let mut cli = make_cli();
        cli.no_locked = true;

        assert!(
            !unit_graph_args(&cli).contains(&"--locked".to_string()),
            "--locked should be omitted when --no-locked is set"
        );
    }

    #[test]
    fn test_unit_graph_args_include_no_run() {
        let args = test_unit_graph_args(&make_cli());
        assert_eq!(args[0], "test");
        assert!(args.contains(&"--unit-graph".to_string()));
        assert!(args.contains(&"--no-run".to_string()));
        assert!(args.contains(&"--locked".to_string()));
    }

    #[test]
    fn cargo_metadata_args_forward_only_feature_flags() {
        let mut cli = make_cli();
        cli.workspace = true;
        cli.features = Some("serde".to_string());
        cli.all_features = true;
        cli.no_default_features = true;
        cli.bin = Some("unit2nix".to_string());
        cli.package = Some("unit2nix".to_string());
        cli.target = Some("aarch64-unknown-linux-gnu".to_string());
        cli.build_std = Some("core,alloc".to_string());

        assert_eq!(
            cargo_metadata_args(&cli),
            vec![
                "metadata",
                "--format-version=1",
                "--locked",
                "--features",
                "serde",
                "--all-features",
                "--no-default-features",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
        );
    }

    // #[test]
    // fn run_cargo_failure_includes_unit_graph_hint() {
    //     let err = with_fake_cargo("echo 'nightly exploded' >&2\nexit 1", || {
    //         run_cargo(
    //             &["build", "--unit-graph"],
    //             Path::new("./Cargo.toml"),
    //             "cargo build --unit-graph",
    //         )
    //         .unwrap_err()
    //         .to_string()
    //     });

    //     assert!(err.contains("nightly exploded"), "got: {err}");
    //     assert!(
    //         err.contains("requires nightly Rust"),
    //         "unit-graph failures should include nightly hint: {err}"
    //     );
    // }

    // #[test]
    // fn run_cargo_failure_includes_stdout_preview() {
    //     let preview = "x".repeat(700);
    //     let script_body = format!("printf '%s' '{preview}'\necho 'bad metadata' >&2\nexit 1");
    //     let err = with_fake_cargo(&script_body, || {
    //         run_cargo(
    //             &["metadata"],
    //             Path::new("./Cargo.toml"),
    //             "cargo metadata",
    //         )
    //         .unwrap_err()
    //         .to_string()
    //     });

    //     assert!(err.contains("bad metadata"), "got: {err}");
    //     assert!(err.contains("stdout (truncated):"), "got: {err}");
    //     assert!(err.contains(&"x".repeat(100)), "stdout preview missing: {err}");
    // }

    #[test]
    fn cargo_lock_hash_is_sha256_hex() {
        // Hash our own Cargo.lock as a smoke test
        let (_lock, hash) =
            read_cargo_lock(Path::new("./Cargo.toml")).expect("should read Cargo.lock");
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
