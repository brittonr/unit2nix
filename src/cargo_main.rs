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
mod source;
mod unit_graph;

use anyhow::{Context, Result};
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

    let cli = Cli::parse_from(filtered);

    eprintln!("Running cargo build --unit-graph...");
    let unit_graph = cargo::run_unit_graph(&cli)?;
    eprintln!("  {} units, {} roots", unit_graph.units.len(), unit_graph.roots.len());

    eprintln!("Running cargo metadata...");
    let metadata = cargo::run_cargo_metadata(&cli)?;
    eprintln!("  {} packages", metadata.packages.len());

    eprintln!("Reading Cargo.lock...");
    let lock = cargo::read_cargo_lock(&cli.manifest_path)?;
    eprintln!(
        "  {} packages with checksums",
        lock.package
            .as_ref()
            .map(|p| p.iter().filter(|p| p.checksum.is_some()).count())
            .unwrap_or(0)
    );

    eprintln!("Hashing Cargo.lock...");
    let cargo_lock_hash = cargo::hash_cargo_lock(&cli.manifest_path)?;
    eprintln!("  sha256: {cargo_lock_hash}");

    eprintln!("Merging...");
    let mut plan = merge::merge(&unit_graph, &metadata, &lock, cli.target.as_deref(), cargo_lock_hash)?;
    eprintln!("  {} crates in build plan", plan.crates.len());
    eprintln!("  {} workspace members", plan.workspace_members.len());
    if let Some(ref t) = plan.target {
        eprintln!("  target: {t}");
    }

    prefetch::prefetch_git_sources(&mut plan)?;

    let json = serde_json::to_string_pretty(&plan)?;

    match &cli.output {
        Some(path) => {
            std::fs::write(path, &json)
                .with_context(|| format!("failed to write output to {}", path.display()))?;
            eprintln!("Wrote {}", path.display());
        }
        None => println!("{json}"),
    }

    Ok(())
}
