use serde::Deserialize;

/// Deserialized output of `cargo build --unit-graph -Z unstable-options`.
///
/// Contains the full dependency graph of compilation units, including
/// resolved features, target kinds, and build/run-custom-build modes.
#[derive(Debug, Deserialize)]
pub struct UnitGraph {
    pub units: Vec<Unit>,
    pub roots: Vec<usize>,
}

/// A single compilation unit in the unit graph.
///
/// Each unit represents one invocation of rustc — a specific crate being
/// compiled in a specific mode (build vs run-custom-build) with specific
/// features enabled.
#[derive(Debug, Deserialize)]
pub struct Unit {
    pub pkg_id: String,
    pub target: UnitTarget,
    pub mode: UnitMode,
    pub features: Vec<String>,
    pub dependencies: Vec<UnitDep>,
}

/// The compilation mode of a unit.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum UnitMode {
    /// Normal compilation: produces a library or binary artifact.
    Build,
    /// Executing a build script: runs build.rs and captures its output.
    RunCustomBuild,
    /// Compiling a crate for testing (lib with `#[cfg(test)]`, or test harness).
    /// Only appears in `cargo test --unit-graph`, not `cargo build --unit-graph`.
    Test,
    /// Running a doctest.
    #[serde(alias = "doctest")]
    Doctest,
    /// Forward-compatible catch-all for any future modes Cargo may add.
    #[serde(other)]
    Other,
}

/// The target being compiled (from `[lib]`, `[[bin]]`, etc).
#[derive(Debug, Deserialize)]
pub struct UnitTarget {
    pub kind: Vec<CrateKind>,
    pub crate_types: Vec<String>,
    pub name: String,
    pub src_path: String,
    pub edition: String,
}

/// The kind of a crate target, as reported by Cargo.
#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum CrateKind {
    Lib,
    Rlib,
    Cdylib,
    Dylib,
    Staticlib,
    ProcMacro,
    Bin,
    CustomBuild,
    /// Forward-compatible catch-all for any future kinds Cargo may add.
    #[serde(other)]
    Other,
}

impl CrateKind {
    /// True for library-like kinds: lib, rlib, cdylib, dylib, staticlib.
    pub fn is_lib(&self) -> bool {
        matches!(self, Self::Lib | Self::Rlib | Self::Cdylib | Self::Dylib | Self::Staticlib)
    }

    /// True if this is a procedural macro.
    pub fn is_proc_macro(&self) -> bool {
        matches!(self, Self::ProcMacro)
    }

    /// True for library-like or proc-macro kinds (targets that produce a linkable artifact).
    pub fn is_lib_like(&self) -> bool {
        self.is_lib() || self.is_proc_macro()
    }
}

impl UnitTarget {
    /// True if any kind is a library type (lib, rlib, cdylib, dylib, staticlib).
    pub fn has_lib(&self) -> bool {
        self.kind.iter().any(CrateKind::is_lib)
    }

    /// True if any kind is proc-macro.
    pub fn has_proc_macro(&self) -> bool {
        self.kind.iter().any(CrateKind::is_proc_macro)
    }

    /// True if any kind is library-like or proc-macro.
    pub fn has_lib_like(&self) -> bool {
        self.kind.iter().any(CrateKind::is_lib_like)
    }

    /// True if any kind is bin.
    pub fn has_bin(&self) -> bool {
        self.kind.iter().any(|k| matches!(k, CrateKind::Bin))
    }

    /// True if any kind is custom-build (build script).
    pub fn has_custom_build(&self) -> bool {
        self.kind.iter().any(|k| matches!(k, CrateKind::CustomBuild))
    }
}

/// A dependency edge between two units in the unit graph.
#[derive(Debug, Deserialize)]
pub struct UnitDep {
    pub index: usize,
    pub extern_crate_name: String,
}
