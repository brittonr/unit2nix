//! Thin entry point for `cargo unit2nix` subcommand.
//!
//! When Cargo invokes `cargo unit2nix -o foo`, it actually runs
//! `cargo-unit2nix unit2nix -o foo` — inserting the subcommand name as the
//! first argument. This wrapper strips that extra argument so clap sees the
//! same flags as a direct `unit2nix` invocation.

// Share all modules with the main binary.
mod cargo;
mod cli;
mod merge;
mod metadata;
mod output;
mod prefetch;
mod run;
mod source;
mod unit_graph;

use anyhow::Result;
use clap::Parser;

use cli::Cli;

fn main() -> Result<()> {
    // Strip the `unit2nix` subcommand arg that Cargo inserts.
    // `cargo unit2nix -o foo` → argv: ["cargo-unit2nix", "unit2nix", "-o", "foo"]
    // We want clap to see: ["cargo-unit2nix", "-o", "foo"]
    let args: Vec<String> = std::env::args().collect();
    let filtered: Vec<&str> = if args.len() > 1 && args[1] == "unit2nix" {
        std::iter::once(args[0].as_str())
            .chain(args[2..].iter().map(|s| s.as_str()))
            .collect()
    } else {
        args.iter().map(|s| s.as_str()).collect()
    };

    run::run(Cli::parse_from(filtered))
}
