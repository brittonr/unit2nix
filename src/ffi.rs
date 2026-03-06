//! C FFI interface for calling from the C++ Nix plugin shim.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::PathBuf;

use crate::cargo;
use crate::cli::Cli;
use crate::merge;
use crate::output::NixBuildPlan;

/// Cargo binary path baked in at build time from the Nix store.
/// Falls back to "cargo" (from `PATH`) when not set.
const BUILTIN_CARGO_PATH: &str = match option_env!("UNIT2NIX_CARGO_PATH") {
    Some(p) => p,
    None => "cargo",
};

/// Static fallback for when `CString::new()` fails (embedded NUL in error message).
const FALLBACK_ERROR: &[u8] = b"internal error: error message contained NUL byte\0";

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
    /// Resolve all workspace members (passes --workspace to cargo)
    #[serde(default)]
    workspace: bool,
}

/// Resolve a Cargo workspace via unit-graph and return a [`NixBuildPlan`].
fn resolve_impl(input: &PluginInput) -> Result<NixBuildPlan, String> {
    // Note: set_var is not thread-safe (POSIX setenv races with getenv).
    // This is acceptable here because the Nix plugin evaluator calls primops
    // from a single-threaded evaluation context.
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
        output: PathBuf::from("build-plan.json"),
        stdout: false,
        check_overrides: false,
        include_dev: input.include_dev,
        members: input.members.clone(),
        no_check: true,
        json: false,
        force: false,
        workspace: input.workspace,
    };

    let members_filter = cli.members_filter();

    // Run cargo commands
    let unit_graph = cargo::run_unit_graph(&cli)
        .map_err(|e| format!("cargo build --unit-graph failed: {e}"))?;

    let metadata = cargo::run_cargo_metadata(&cli)
        .map_err(|e| format!("cargo metadata failed: {e}"))?;

    let (lock, cargo_lock_hash) = cargo::read_cargo_lock(&cli.manifest_path)
        .map_err(|e| format!("failed to read Cargo.lock: {e}"))?;

    // --workspace implies --include-dev
    let include_dev = cli.include_dev || cli.workspace;

    let test_unit_graph = if include_dev {
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

/// Create a `CString` from a string, falling back to a static error message
/// if the string contains an embedded NUL byte. Prevents panicking in FFI.
fn cstring_or_fallback(s: String) -> CString {
    CString::new(s).unwrap_or_else(|_| {
        // SAFETY: FALLBACK_ERROR is a valid NUL-terminated byte string with no interior NULs.
        unsafe { CString::from_vec_unchecked(FALLBACK_ERROR[..FALLBACK_ERROR.len() - 1].to_vec()) }
    })
}

/// Resolve a Cargo workspace. Input and output are JSON strings.
///
/// # Safety
/// `input_json` must be a valid null-terminated C string.
/// The returned strings must be freed with `free_string`.
///
/// # Errors
/// Returns 1 and sets `err_out` on invalid input, parse failure, or resolution error.
///
/// # Panics
/// Does not panic — all error paths use `cstring_or_fallback` to avoid
/// panicking across the FFI boundary.
#[no_mangle]
pub unsafe extern "C" fn resolve_unit_graph(
    input_json: *const c_char,
    out: *mut *mut c_char,
    err_out: *mut *mut c_char,
) -> i32 {
    let input_str = match unsafe { CStr::from_ptr(input_json) }.to_str() {
        Ok(s) => s,
        Err(e) => {
            let msg = cstring_or_fallback(format!("Invalid UTF-8 in input: {e}"));
            unsafe { *err_out = msg.into_raw() };
            return 1;
        }
    };

    let input: PluginInput = match serde_json::from_str(input_str) {
        Ok(v) => v,
        Err(e) => {
            let msg = cstring_or_fallback(format!("Failed to parse plugin input: {e}"));
            unsafe { *err_out = msg.into_raw() };
            return 1;
        }
    };

    match resolve_impl(&input) {
        Ok(plan) => {
            let json = match serde_json::to_string(&plan) {
                Ok(j) => j,
                Err(e) => {
                    let msg = cstring_or_fallback(format!("Failed to serialize output: {e}"));
                    unsafe { *err_out = msg.into_raw() };
                    return 1;
                }
            };
            let cstr = cstring_or_fallback(json);
            unsafe { *out = cstr.into_raw() };
            0
        }
        Err(e) => {
            let msg = cstring_or_fallback(e);
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
