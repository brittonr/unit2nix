use anyhow::{Context, Result};

use crate::cargo;
use crate::cli::Cli;
use crate::merge;
use crate::prefetch;

/// Shared entry point for both `unit2nix` and `cargo unit2nix`.
pub fn run(cli: Cli) -> Result<()> {
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

    // Prefetch git sources for pure flake evaluation
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
