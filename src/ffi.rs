//! C FFI interface for calling from the C++ Nix plugin shim.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::PathBuf;

use crate::cargo;
use crate::cli::Cli;
use crate::merge;
use crate::output::NixBuildPlan;

/// Cargo binary path baked in at build time from the Nix store.
/// Falls back to "cargo" (from PATH) when not set.
const BUILTIN_CARGO_PATH: &str = match option_env!("UNIT2NIX_CARGO_PATH") {
    Some(p) => p,
    None => "cargo",
};

/// Input from the Nix side — the entire attrset serialized as JSON.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginInput {
    /// Path to Cargo.toml
    manifest_path: String,
    /// Optional target triple
    #[serde(default)]
    target: Option<String>,
    /// Include dev dependencies (runs cargo test --unit-graph)
    #[serde(default)]
    include_dev: bool,
    /// Features to enable (comma-separated)
    #[serde(default)]
    features: Option<String>,
    /// Enable all features
    #[serde(default)]
    all_features: bool,
    /// Disable default features
    #[serde(default)]
    no_default_features: bool,
    /// Build a specific binary
    #[serde(default)]
    bin: Option<String>,
    /// Build a specific package
    #[serde(default)]
    package: Option<String>,
    /// Build only specific workspace members (comma-separated)
    #[serde(default)]
    members: Option<String>,
}

/// Resolve a Cargo workspace via unit-graph and return a NixBuildPlan.
fn resolve_impl(input: &PluginInput) -> Result<NixBuildPlan, String> {
    // Set CARGO env var so cargo::run_cargo uses the baked-in path
    std::env::set_var("CARGO", BUILTIN_CARGO_PATH);

    // Construct a Cli struct to pass to cargo functions
    let cli = Cli {
        manifest_path: PathBuf::from(&input.manifest_path),
        features: input.features.clone(),
        bin: input.bin.clone(),
        package: input.package.clone(),
        all_features: input.all_features,
        no_default_features: input.no_default_features,
        target: input.target.clone(),
        output: None,
        check_overrides: false,
        include_dev: input.include_dev,
        members: input.members.clone(),
        no_check: true,
        json: false,
    };

    // Parse members filter
    let members_filter: Option<Vec<String>> = cli.members.as_ref().map(|m| {
        m.split(',').map(|s| s.trim().to_string()).collect()
    });

    // Run cargo commands
    let unit_graph = cargo::run_unit_graph(&cli)
        .map_err(|e| format!("cargo build --unit-graph failed: {e}"))?;

    let metadata = cargo::run_cargo_metadata(&cli)
        .map_err(|e| format!("cargo metadata failed: {e}"))?;

    let lock = cargo::read_cargo_lock(&cli.manifest_path)
        .map_err(|e| format!("failed to read Cargo.lock: {e}"))?;

    let cargo_lock_hash = cargo::hash_cargo_lock(&cli.manifest_path)
        .map_err(|e| format!("failed to hash Cargo.lock: {e}"))?;

    let test_unit_graph = if cli.include_dev {
        Some(
            cargo::run_test_unit_graph(&cli)
                .map_err(|e| format!("cargo test --unit-graph failed: {e}"))?,
        )
    } else {
        None
    };

    // Merge
    let plan = merge::merge(
        &unit_graph,
        &metadata,
        &lock,
        cli.target.as_deref(),
        cargo_lock_hash,
        test_unit_graph.as_ref(),
        members_filter.as_deref(),
    )
    .map_err(|e| format!("merge failed: {e}"))?;

    // Skip prefetch — the Nix wrapper handles git sources

    Ok(plan)
}

/// Resolve a Cargo workspace. Input and output are JSON strings.
///
/// # Safety
/// `input_json` must be a valid null-terminated C string.
/// The returned strings must be freed with `free_string`.
#[no_mangle]
pub unsafe extern "C" fn resolve_unit_graph(
    input_json: *const c_char,
    out: *mut *mut c_char,
    err_out: *mut *mut c_char,
) -> i32 {
    let input_str = match unsafe { CStr::from_ptr(input_json) }.to_str() {
        Ok(s) => s,
        Err(e) => {
            let msg = CString::new(format!("Invalid UTF-8 in input: {e}")).unwrap();
            unsafe { *err_out = msg.into_raw() };
            return 1;
        }
    };

    let input: PluginInput = match serde_json::from_str(input_str) {
        Ok(v) => v,
        Err(e) => {
            let msg = CString::new(format!("Failed to parse plugin input: {e}")).unwrap();
            unsafe { *err_out = msg.into_raw() };
            return 1;
        }
    };

    match resolve_impl(&input) {
        Ok(plan) => {
            let json = serde_json::to_string(&plan).unwrap();
            let cstr = CString::new(json).unwrap();
            unsafe { *out = cstr.into_raw() };
            0
        }
        Err(e) => {
            let msg = CString::new(e).unwrap();
            unsafe { *err_out = msg.into_raw() };
            1
        }
    }
}

/// Free a string returned by `resolve_unit_graph`.
///
/// # Safety
/// The pointer must have been returned by `resolve_unit_graph`.
#[no_mangle]
pub unsafe extern "C" fn free_string(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}
