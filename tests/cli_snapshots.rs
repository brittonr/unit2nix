use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use serde_json::{json, Value};
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

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("failed to parse {} as json: {e}", path.display()))
}

fn generated_plan_json() -> Value {
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
    assert_success(&output, "sample workspace build-plan generation");
    read_json(&output_path)
}

fn generated_members_plan_json() -> Value {
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
    assert_success(&output, "sample workspace filtered build-plan generation");
    read_json(&output_path)
}

fn override_report_json() -> Value {
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
    assert_success(&generate, "plan generation for override snapshot");

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
    assert_success(&output, "override report generation");
    serde_json::from_slice(&output.stdout).expect("override report should be json")
}

fn sanitize_plan(plan: &Value) -> Value {
    let members = object_entries(&plan["workspaceMembers"]);

    let roots = string_array(&plan["roots"]);

    let crates = plan["crates"]
        .as_object()
        .expect("crates should be an object")
        .values()
        .map(sanitize_crate)
        .collect::<Vec<_>>();

    json!({
        "version": plan["version"],
        "target": plan["target"],
        "workspaceMembers": members,
        "roots": roots,
        "crateCount": plan["crates"].as_object().unwrap().len(),
        "crates": crates,
    })
}

fn sanitize_crate(crate_info: &Value) -> Value {
    json!({
        "crateName": crate_info["crateName"],
        "source": normalize_json(&crate_info["source"]),
        "features": string_array(&crate_info["features"]),
        "hostFeatures": string_array_or_null(&crate_info["hostFeatures"]),
        "dependencies": dep_names(&crate_info["dependencies"]),
        "buildDependencies": dep_names(&crate_info["buildDependencies"]),
        "devDependencies": dep_names(&crate_info["devDependencies"]),
        "procMacro": crate_info["procMacro"],
        "build": crate_info["build"],
        "libPath": crate_info["libPath"],
        "libName": crate_info["libName"],
        "crateBin": bin_targets(&crate_info["crateBin"]),
        "links": crate_info["links"],
    })
}

fn dep_names(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .map(|dep| {
            json!({
                "externCrateName": dep["externCrateName"],
                "packageId": normalize_json(&dep["packageId"]),
            })
        })
        .collect()
}

fn bin_targets(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .map(|bin| {
            json!({
                "name": bin["name"],
                "path": bin["path"],
            })
        })
        .collect()
}

fn string_array(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .expect("value should be an array")
        .iter()
        .map(normalize_json)
        .collect()
}

fn string_array_or_null(value: &Value) -> Value {
    if value.is_null() {
        Value::Null
    } else {
        Value::Array(string_array(value))
    }
}

fn object_entries(value: &Value) -> BTreeMap<String, Value> {
    value
        .as_object()
        .expect("value should be an object")
        .iter()
        .map(|(k, v)| (k.clone(), normalize_json(v)))
        .collect()
}

fn normalize_json(value: &Value) -> Value {
    match value {
        Value::String(s) => Value::String(normalize_string(s)),
        Value::Array(items) => Value::Array(items.iter().map(normalize_json).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), normalize_json(v)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn normalize_string(value: &str) -> String {
    let repo = repo_root();
    let repo_str = repo.to_string_lossy();
    value.replace(repo_str.as_ref(), "<repo>")
}

fn assert_snapshot(name: &str, actual: &Value) {
    let path = repo_root().join("tests/snapshots").join(name);
    let expected_text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read snapshot {}: {e}", path.display()));
    let expected: Value = serde_json::from_str(&expected_text)
        .unwrap_or_else(|e| panic!("failed to parse snapshot {}: {e}", path.display()));
    assert_eq!(actual, &expected, "snapshot mismatch for {}", path.display());
}

#[test]
fn sample_workspace_build_plan_snapshot_matches() {
    let actual = sanitize_plan(&generated_plan_json());
    assert_snapshot("sample-workspace-plan.json", &actual);
}

#[test]
fn sample_workspace_override_report_snapshot_matches() {
    let actual = override_report_json();
    assert_snapshot("sample-workspace-overrides.json", &actual);
}

#[test]
fn sample_workspace_members_filter_snapshot_matches() {
    let actual = sanitize_plan(&generated_members_plan_json());
    assert_snapshot("sample-workspace-members-plan.json", &actual);
}
