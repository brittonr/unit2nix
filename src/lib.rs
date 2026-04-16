//! unit2nix library — generate per-crate Nix build plans from Cargo's unit graph.
//!
//! This library is used by both the CLI binary and the optional Nix plugin.

pub mod cargo;
pub mod cli;
pub mod fingerprint;
pub mod merge;
pub mod metadata;
pub mod output;
pub mod overrides;
pub mod prefetch;
pub mod run;
pub mod source;
pub mod unit_graph;

#[cfg(feature = "ffi")]
pub mod ffi;

#[cfg(test)]
pub mod test_support {
    use std::sync::{Mutex, OnceLock};

    pub fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }
}
