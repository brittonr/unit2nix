use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use tempfile::tempdir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn sample_manifest() -> PathBuf {
    repo_root().join("sample_workspace/Cargo.toml")
}

fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target"))
}

fn build_bins_once() {
    static BUILD: OnceLock<()> = OnceLock::new();
    BUILD.get_or_init(|| {
        let output = Command::new(cargo_bin())
            .args(["build", "--bins", "--quiet"])
            .current_dir(repo_root())
            .output()
            .unwrap_or_else(|e| panic!("failed to build test binaries: {e}"));
        assert_success(&output, "cargo build --bins --quiet");
    });
}

fn unit2nix_bin() -> PathBuf {
    build_bins_once();
    target_dir().join("debug/unit2nix")
}

fn cargo_unit2nix_bin() -> PathBuf {
    build_bins_once();
    target_dir().join("debug/cargo-unit2nix")
}

fn run_command(bin: &Path, args: &[&str], current_dir: &Path) -> Output {
    Command::new(bin)
        .args(args)
        .current_dir(current_dir)
        .output()
        .unwrap_or_else(|e| panic!("failed to run {}: {e}", bin.display()))
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_failure(output: &Output, expected_stderr: &str) {
    assert!(
        !output.status.success(),
        "command unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(expected_stderr),
        "stderr missing expected text {expected_stderr:?}\nstderr:\n{stderr}"
    );
}

fn read_json(path: &Path) -> serde_json::Value {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("failed to parse {} as json: {e}", path.display()))
}

fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .unwrap_or_else(|e| panic!("failed to create {}: {e}", parent.display()));
    }
    fs::write(path, content)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", path.display()));
}

#[test]
fn unit2nix_writes_build_plan_json() {
    let temp = tempdir().unwrap();
    let output_path = temp.path().join("build-plan.json");
    let manifest = sample_manifest();

    let output = run_command(
        &unit2nix_bin(),
        &[
            "--manifest-path",
            manifest.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
            "--no-check",
        ],
        &repo_root(),
    );
    assert_success(&output, "unit2nix build plan generation");

    let plan = read_json(&output_path);
    let members = plan["workspaceMembers"].as_object().unwrap();
    assert_eq!(members.len(), 4, "sample workspace should expose 4 members");
    assert!(members.contains_key("sample-bin"));
    assert!(plan["inputsHash"].is_string(), "generated plan should record inputsHash");
}

#[test]
fn cargo_unit2nix_strips_inserted_subcommand_arg() {
    let temp = tempdir().unwrap();
    let output_path = temp.path().join("build-plan.json");
    let manifest = sample_manifest();

    let output = run_command(
        &cargo_unit2nix_bin(),
        &[
            "unit2nix",
            "--manifest-path",
            manifest.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
            "--no-check",
        ],
        &repo_root(),
    );
    assert_success(&output, "cargo-unit2nix wrapper invocation");

    let plan = read_json(&output_path);
    assert!(plan["crates"].is_object(), "plan json should contain crates map");
}

#[test]
fn stdout_mode_emits_json_and_does_not_write_default_file() {
    let temp = tempdir().unwrap();
    let manifest = sample_manifest();

    let output = run_command(
        &unit2nix_bin(),
        &[
            "--manifest-path",
            manifest.to_str().unwrap(),
            "--stdout",
            "--no-check",
        ],
        temp.path(),
    );
    assert_success(&output, "unit2nix stdout mode");

    let stdout = String::from_utf8(output.stdout).unwrap();
    let plan: serde_json::Value = serde_json::from_str(&stdout).expect("stdout should be valid json");
    assert!(plan["workspaceMembers"].is_object());
    assert!(
        !temp.path().join("build-plan.json").exists(),
        "--stdout should not create build-plan.json"
    );
}

#[test]
fn members_filter_restricts_workspace_members() {
    let temp = tempdir().unwrap();
    let output_path = temp.path().join("build-plan.json");
    let manifest = sample_manifest();

    let output = run_command(
        &unit2nix_bin(),
        &[
            "--manifest-path",
            manifest.to_str().unwrap(),
            "--members",
            "sample-bin,sample-lib",
            "-o",
            output_path.to_str().unwrap(),
            "--no-check",
        ],
        &repo_root(),
    );
    assert_success(&output, "unit2nix members filter");

    let plan = read_json(&output_path);
    let members = plan["workspaceMembers"].as_object().unwrap();
    let keys: Vec<_> = members.keys().cloned().collect();
    assert_eq!(keys, vec!["sample-bin", "sample-lib"]);
}

#[test]
fn workspace_flag_captures_dev_dependencies_for_all_members() {
    let temp = tempdir().unwrap();
    let output_path = temp.path().join("build-plan.json");
    let manifest = sample_manifest();

    let output = run_command(
        &unit2nix_bin(),
        &[
            "--manifest-path",
            manifest.to_str().unwrap(),
            "--workspace",
            "-o",
            output_path.to_str().unwrap(),
            "--no-check",
        ],
        &repo_root(),
    );
    assert_success(&output, "unit2nix workspace mode");

    let plan = read_json(&output_path);
    let crates = plan["crates"].as_object().unwrap();
    let sample_lib = crates
        .values()
        .find(|crate_info| crate_info["crateName"] == "sample-lib")
        .expect("sample-lib crate should exist");
    let sample_bin = crates
        .values()
        .find(|crate_info| crate_info["crateName"] == "sample-bin")
        .expect("sample-bin crate should exist");

    assert!(
        !sample_lib["devDependencies"].as_array().unwrap().is_empty(),
        "--workspace should include sample-lib devDependencies"
    );
    assert!(
        !sample_bin["devDependencies"].as_array().unwrap().is_empty(),
        "--workspace should include sample-bin devDependencies"
    );
}

#[test]
fn check_overrides_json_reads_existing_plan() {
    let temp = tempdir().unwrap();
    let output_path = temp.path().join("build-plan.json");
    let manifest = sample_manifest();

    let generate = run_command(
        &unit2nix_bin(),
        &[
            "--manifest-path",
            manifest.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
            "--no-check",
        ],
        &repo_root(),
    );
    assert_success(&generate, "build plan generation for check-overrides");

    let output = run_command(
        &unit2nix_bin(),
        &[
            "--check-overrides",
            "--json",
            "-o",
            output_path.to_str().unwrap(),
        ],
        &repo_root(),
    );
    assert_success(&output, "check-overrides json report");

    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["total"], 0, "sample workspace is pure Rust");
    assert_eq!(report["missing"], 0);
    assert!(report["crates"].as_array().unwrap().is_empty());
}

#[test]
fn second_run_without_force_reports_up_to_date() {
    let temp = tempdir().unwrap();
    let output_path = temp.path().join("build-plan.json");
    let manifest = sample_manifest();

    let first = run_command(
        &unit2nix_bin(),
        &[
            "--manifest-path",
            manifest.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
            "--no-check",
        ],
        &repo_root(),
    );
    assert_success(&first, "initial generation");
    let original = fs::read_to_string(&output_path).unwrap();

    let second = run_command(
        &unit2nix_bin(),
        &[
            "--manifest-path",
            manifest.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
            "--no-check",
        ],
        &repo_root(),
    );
    assert_success(&second, "second generation");

    let stderr = String::from_utf8(second.stderr).unwrap();
    assert!(
        stderr.contains("Build plan is up to date"),
        "second run should report fingerprint hit, stderr:\n{stderr}"
    );
    let unchanged = fs::read_to_string(&output_path).unwrap();
    assert_eq!(unchanged, original, "fingerprint hit should keep file content unchanged");
}

#[test]
fn force_bypasses_fingerprint_skip() {
    let temp = tempdir().unwrap();
    let output_path = temp.path().join("build-plan.json");
    let manifest = sample_manifest();

    let first = run_command(
        &unit2nix_bin(),
        &[
            "--manifest-path",
            manifest.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
            "--no-check",
        ],
        &repo_root(),
    );
    assert_success(&first, "initial generation for --force");

    let forced = run_command(
        &unit2nix_bin(),
        &[
            "--manifest-path",
            manifest.to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
            "--force",
            "--no-check",
        ],
        &repo_root(),
    );
    assert_success(&forced, "forced regeneration");

    let stderr = String::from_utf8(forced.stderr).unwrap();
    assert!(
        !stderr.contains("Build plan is up to date"),
        "--force should bypass fingerprint skip, stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Running cargo build --unit-graph"),
        "--force should regenerate plan, stderr:\n{stderr}"
    );
}

#[test]
fn out_of_tree_git_path_dependency_becomes_git_source() {
    let temp = tempdir().unwrap();
    let sibling = temp.path().join("sibling-crate");
    write_file(
        &sibling.join("Cargo.toml"),
        "[package]\nname = \"sibling-crate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write_file(
        &sibling.join("src/lib.rs"),
        "pub fn greeting() -> &'static str { \"hello\" }\n",
    );

    let git_init = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&sibling)
        .output()
        .unwrap();
    assert_success(&git_init, "git init sibling repo");
    assert_success(
        &Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&sibling)
            .output()
            .unwrap(),
        "git config user.email",
    );
    assert_success(
        &Command::new("git")
            .args(["config", "user.name", "Unit2nix Tests"])
            .current_dir(&sibling)
            .output()
            .unwrap(),
        "git config user.name",
    );
    assert_success(
        &Command::new("git")
            .args(["add", "."])
            .current_dir(&sibling)
            .output()
            .unwrap(),
        "git add sibling repo",
    );
    assert_success(
        &Command::new("git")
            .args(["commit", "-qm", "init"])
            .current_dir(&sibling)
            .output()
            .unwrap(),
        "git commit sibling repo",
    );
    assert_success(
        &Command::new("git")
            .args(["remote", "add", "origin", "https://example.com/sibling-crate.git"])
            .current_dir(&sibling)
            .output()
            .unwrap(),
        "git remote add origin",
    );

    let app = temp.path().join("app");
    write_file(
        &app.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nsibling-crate = { path = \"../sibling-crate\" }\n",
    );
    write_file(
        &app.join("src/main.rs"),
        "fn main() { println!(\"{}\", sibling_crate::greeting()); }\n",
    );
    assert_success(
        &Command::new(cargo_bin())
            .args(["generate-lockfile"])
            .current_dir(&app)
            .output()
            .unwrap(),
        "cargo generate-lockfile for out-of-tree fixture",
    );

    let output_path = temp.path().join("build-plan.json");
    let output = run_command(
        &unit2nix_bin(),
        &[
            "--manifest-path",
            app.join("Cargo.toml").to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
            "--no-check",
        ],
        &repo_root(),
    );
    assert_success(&output, "unit2nix out-of-tree git path dependency fixture");

    let plan = read_json(&output_path);
    let sibling_crate = plan["crates"]
        .as_object()
        .unwrap()
        .values()
        .find(|crate_info| crate_info["crateName"] == "sibling-crate")
        .expect("sibling-crate should be present in build plan");

    assert_eq!(sibling_crate["source"]["type"], "git");
    assert_eq!(
        sibling_crate["source"]["url"],
        "https://example.com/sibling-crate.git"
    );
    assert!(
        sibling_crate["source"]["rev"].as_str().unwrap().len() >= 7,
        "git source should include a commit rev"
    );
}

#[test]
fn out_of_tree_non_git_path_dependency_stays_local() {
    let temp = tempfile::Builder::new()
        .prefix("unit2nix-cli-")
        .tempdir_in("/tmp")
        .expect("should create tempdir outside repo git root");
    let sibling = temp.path().join("plain-sibling");
    write_file(
        &sibling.join("Cargo.toml"),
        "[package]\nname = \"plain-sibling\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    write_file(
        &sibling.join("src/lib.rs"),
        "pub fn answer() -> u32 { 42 }\n",
    );

    let app = temp.path().join("app");
    write_file(
        &app.join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nplain-sibling = { path = \"../plain-sibling\" }\n",
    );
    write_file(
        &app.join("src/main.rs"),
        "fn main() { println!(\"{}\", plain_sibling::answer()); }\n",
    );
    assert_success(
        &Command::new(cargo_bin())
            .args(["generate-lockfile"])
            .current_dir(&app)
            .output()
            .unwrap(),
        "cargo generate-lockfile for plain out-of-tree fixture",
    );

    let output_path = temp.path().join("build-plan.json");
    let output = run_command(
        &unit2nix_bin(),
        &[
            "--manifest-path",
            app.join("Cargo.toml").to_str().unwrap(),
            "-o",
            output_path.to_str().unwrap(),
            "--no-check",
        ],
        &repo_root(),
    );
    assert_success(&output, "unit2nix plain out-of-tree path dependency fixture");

    let plan = read_json(&output_path);
    let sibling_crate = plan["crates"]
        .as_object()
        .unwrap()
        .values()
        .find(|crate_info| crate_info["crateName"] == "plain-sibling")
        .expect("plain-sibling should be present in build plan");

    assert_eq!(sibling_crate["source"]["type"], "local");
    assert_eq!(
        sibling_crate["source"]["path"],
        sibling.to_string_lossy().into_owned()
    );
}

#[test]
fn build_std_fixture_generates_stdlib_sources_or_reports_missing_rust_src() {
    let temp = tempdir().unwrap();
    let fixture = temp.path().join("nostd-sample");
    write_file(
        &fixture.join("Cargo.toml"),
        "[package]\nname = \"nostd-sample\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"rlib\"]\n",
    );
    write_file(
        &fixture.join("src/lib.rs"),
        "#![no_std]\n\npub fn add(a: u32, b: u32) -> u32 { a + b }\n",
    );
    assert_success(
        &Command::new(cargo_bin())
            .args(["generate-lockfile"])
            .current_dir(&fixture)
            .output()
            .unwrap(),
        "cargo generate-lockfile for build-std fixture",
    );

    let output_path = temp.path().join("build-plan.json");
    let output = run_command(
        &unit2nix_bin(),
        &[
            "--manifest-path",
            fixture.join("Cargo.toml").to_str().unwrap(),
            "--build-std",
            "core,alloc",
            "-o",
            output_path.to_str().unwrap(),
            "--no-check",
        ],
        &repo_root(),
    );

    if output.status.success() {
        let plan = read_json(&output_path);
        let stdlib_crates: Vec<_> = plan["crates"]
            .as_object()
            .unwrap()
            .values()
            .filter(|crate_info| crate_info["source"]["type"] == "stdlib")
            .map(|crate_info| crate_info["crateName"].as_str().unwrap().to_string())
            .collect();
        assert!(
            stdlib_crates.iter().any(|name| name == "core" || name == "alloc"),
            "successful --build-std run should include stdlib crates, got: {stdlib_crates:?}"
        );
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("rust-src") || stderr.contains("standard library"),
            "unexpected --build-std failure:\n{stderr}"
        );
        assert!(
            stderr.contains("requires nightly Rust"),
            "unit-graph failure path should keep nightly hint:\n{stderr}"
        );
    }
}

#[test]
fn invalid_members_name_fails_fast() {
    let temp = tempdir().unwrap();
    let manifest = sample_manifest();
    let output = run_command(
        &unit2nix_bin(),
        &[
            "--manifest-path",
            manifest.to_str().unwrap(),
            "--members",
            "not-a-member",
            "-o",
            temp.path().join("build-plan.json").to_str().unwrap(),
            "--no-check",
        ],
        &repo_root(),
    );

    assert_failure(&output, "unknown workspace member 'not-a-member'");
}

#[test]
fn check_overrides_human_output_reports_pure_rust_plan() {
    let temp = tempdir().unwrap();
    let plan_path = temp.path().join("plan.json");
    let plan = serde_json::json!({
        "version": 1,
        "workspaceRoot": "/workspace",
        "roots": ["pkg#0.1.0"],
        "workspaceMembers": {"pkg": "pkg#0.1.0"},
        "cargoLockHash": "hash",
        "crates": {
            "pkg#0.1.0": {
                "crateName": "pkg",
                "version": "0.1.0",
                "edition": "2021",
                "sha256": null,
                "source": {"type": "local", "path": "."},
                "features": [],
                "dependencies": [],
                "buildDependencies": [],
                "procMacro": false,
                "build": null,
                "libPath": null,
                "libName": null,
                "libCrateTypes": ["lib"],
                "crateBin": [],
                "links": null
            }
        }
    });
    write_file(&plan_path, &serde_json::to_string_pretty(&plan).unwrap());

    let output = run_command(
        &unit2nix_bin(),
        &[
            "--check-overrides",
            "-o",
            plan_path.to_str().unwrap(),
        ],
        &repo_root(),
    );
    assert_success(&output, "check-overrides pure rust output");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("No crates with native link requirements found."), "stdout:\n{stdout}");
    assert!(stdout.contains("pure Rust project"), "stdout:\n{stdout}");
}

#[test]
fn check_overrides_human_output_reports_unknown_links_crates() {
    let temp = tempdir().unwrap();
    let plan_path = temp.path().join("plan.json");
    let plan = serde_json::json!({
        "version": 1,
        "workspaceRoot": "/workspace",
        "roots": ["pkg#0.1.0"],
        "workspaceMembers": {"pkg": "pkg#0.1.0"},
        "cargoLockHash": "hash",
        "crates": {
            "pkg#0.1.0": {
                "crateName": "pkg",
                "version": "0.1.0",
                "edition": "2021",
                "sha256": null,
                "source": {"type": "local", "path": "."},
                "features": [],
                "dependencies": [],
                "buildDependencies": [],
                "procMacro": false,
                "build": null,
                "libPath": null,
                "libName": null,
                "libCrateTypes": ["lib"],
                "crateBin": [],
                "links": null
            },
            "ring#0.1.0": {
                "crateName": "ring",
                "version": "0.1.0",
                "edition": "2021",
                "sha256": null,
                "source": {"type": "crates-io"},
                "features": [],
                "dependencies": [],
                "buildDependencies": [],
                "procMacro": false,
                "build": null,
                "libPath": null,
                "libName": null,
                "libCrateTypes": ["lib"],
                "crateBin": [],
                "links": "ring_core_0_17_14_"
            },
            "my-custom-sys#1.0.0": {
                "crateName": "my-custom-sys",
                "version": "1.0.0",
                "edition": "2021",
                "sha256": null,
                "source": {"type": "crates-io"},
                "features": [],
                "dependencies": [],
                "buildDependencies": [],
                "procMacro": false,
                "build": null,
                "libPath": null,
                "libName": null,
                "libCrateTypes": ["lib"],
                "crateBin": [],
                "links": "my_custom"
            }
        }
    });
    write_file(&plan_path, &serde_json::to_string_pretty(&plan).unwrap());

    let output = run_command(
        &unit2nix_bin(),
        &[
            "--check-overrides",
            "-o",
            plan_path.to_str().unwrap(),
        ],
        &repo_root(),
    );
    assert_success(&output, "check-overrides unknown crate output");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("my-custom-sys"), "stdout:\n{stdout}");
    assert!(stdout.contains("ring"), "stdout:\n{stdout}");
    assert!(stdout.contains("may need attention"), "stdout:\n{stdout}");
    assert!(stdout.contains("extraCrateOverrides"), "stdout:\n{stdout}");
}

#[test]
fn check_overrides_human_output_reports_covered_links_crates() {
    let temp = tempdir().unwrap();
    let plan_path = temp.path().join("plan.json");
    let plan = serde_json::json!({
        "version": 1,
        "workspaceRoot": "/workspace",
        "roots": ["libz-sys#1.0.0"],
        "workspaceMembers": {"libz-sys": "libz-sys#1.0.0"},
        "cargoLockHash": "hash",
        "crates": {
            "libz-sys#1.0.0": {
                "crateName": "libz-sys",
                "version": "1.0.0",
                "edition": "2021",
                "sha256": null,
                "source": {"type": "crates-io"},
                "features": [],
                "dependencies": [],
                "buildDependencies": [],
                "procMacro": false,
                "build": null,
                "libPath": null,
                "libName": null,
                "libCrateTypes": ["lib"],
                "crateBin": [],
                "links": "z"
            }
        }
    });
    write_file(&plan_path, &serde_json::to_string_pretty(&plan).unwrap());

    let output = run_command(
        &unit2nix_bin(),
        &[
            "--check-overrides",
            "-o",
            plan_path.to_str().unwrap(),
        ],
        &repo_root(),
    );
    assert_success(&output, "check-overrides covered crate output");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("libz-sys"), "stdout:\n{stdout}");
    assert!(stdout.contains("covered"), "stdout:\n{stdout}");
    assert!(stdout.contains("pkg-config + zlib"), "stdout:\n{stdout}");
}

#[test]
fn check_overrides_missing_plan_fails_with_read_error() {
    let temp = tempdir().unwrap();
    let missing = temp.path().join("missing-plan.json");
    let output = run_command(
        &unit2nix_bin(),
        &[
            "--check-overrides",
            "-o",
            missing.to_str().unwrap(),
        ],
        &repo_root(),
    );

    assert_failure(&output, "failed to read build plan");
}

#[test]
fn check_overrides_invalid_json_fails_with_parse_error() {
    let temp = tempdir().unwrap();
    let invalid = temp.path().join("invalid-plan.json");
    write_file(&invalid, "not valid json\n");
    let output = run_command(
        &unit2nix_bin(),
        &[
            "--check-overrides",
            "-o",
            invalid.to_str().unwrap(),
        ],
        &repo_root(),
    );

    assert_failure(&output, "failed to parse build plan");
}

#[test]
fn workspace_and_package_flags_fail_fast() {
    let manifest = sample_manifest();
    let output = run_command(
        &unit2nix_bin(),
        &[
            "--manifest-path",
            manifest.to_str().unwrap(),
            "--workspace",
            "--package",
            "sample-bin",
        ],
        &repo_root(),
    );

    assert_failure(&output, "--workspace and --package cannot be used together");
}
