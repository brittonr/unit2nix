use anyhow::{Context, Result};

use crate::cargo;
use crate::cli::Cli;
use crate::merge;
use crate::output::NixBuildPlan;
use crate::overrides;
use crate::prefetch;

/// Shared entry point for both `unit2nix` and `cargo unit2nix`.
pub fn run(cli: Cli) -> Result<()> {
    // --check-overrides: read an existing build plan and report coverage
    if cli.check_overrides {
        return run_check_overrides(&cli);
    }

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

    let test_unit_graph = if cli.include_dev {
        eprintln!("Running cargo test --unit-graph (for dev dependencies)...");
        let tug = cargo::run_test_unit_graph(&cli)?;
        eprintln!("  {} units, {} roots", tug.units.len(), tug.roots.len());
        Some(tug)
    } else {
        None
    };

    eprintln!("Merging...");
    let mut plan = merge::merge(
        &unit_graph,
        &metadata,
        &lock,
        cli.target.as_deref(),
        cargo_lock_hash,
        test_unit_graph.as_ref(),
    )?;
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

/// Read an existing build plan JSON and check override coverage.
fn run_check_overrides(cli: &Cli) -> Result<()> {
    let path = cli
        .output
        .as_ref()
        .context("--check-overrides requires -o <build-plan.json> to specify which plan to check")?;

    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read build plan from {}", path.display()))?;

    let plan: NixBuildPlan = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse build plan from {}", path.display()))?;

    overrides::check_overrides(&plan);
    Ok(())
}
