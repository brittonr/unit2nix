//! unit2nix library — generate per-crate Nix build plans from Cargo's unit graph.
//!
//! This library is used by both the CLI binary and the optional Nix plugin.

pub mod cargo;
pub mod cli;
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
