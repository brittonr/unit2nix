use anyhow::{bail, Context, Result};

use crate::cargo;
use crate::cli::Cli;
use crate::merge;
use crate::output::NixBuildPlan;
use crate::overrides::{self, print_override_report};
use crate::prefetch;

/// Shared entry point for both `unit2nix` and `cargo unit2nix`.
pub fn run(cli: &Cli) -> Result<()> {
    // --check-overrides: read an existing build plan and report coverage
    if cli.check_overrides {
        return run_check_overrides(cli);
    }

    // Validate: --members and --package are mutually exclusive
    if cli.members.is_some() && cli.package.is_some() {
        bail!("--members and --package cannot be used together");
    }

    let members_filter: Option<Vec<String>> = cli.members.as_ref().map(|m| {
        m.split(',').map(|s| s.trim().to_string()).collect()
    });

    eprintln!("Running cargo build --unit-graph...");
    let unit_graph = cargo::run_unit_graph(cli)?;
    eprintln!("  {} units, {} roots", unit_graph.units.len(), unit_graph.roots.len());

    eprintln!("Running cargo metadata...");
    let metadata = cargo::run_cargo_metadata(cli)?;
    eprintln!("  {} packages", metadata.packages.len());

    eprintln!("Reading Cargo.lock...");
    let lock = cargo::read_cargo_lock(&cli.manifest_path)?;
    eprintln!(
        "  {} packages with checksums",
        lock.package
            .as_ref()
            .map_or(0, |p| p.iter().filter(|p| p.checksum.is_some()).count())
    );

    eprintln!("Hashing Cargo.lock...");
    let cargo_lock_hash = cargo::hash_cargo_lock(&cli.manifest_path)?;
    eprintln!("  sha256: {cargo_lock_hash}");

    let test_unit_graph = if cli.include_dev {
        eprintln!("Running cargo test --unit-graph (for dev dependencies)...");
        let tug = cargo::run_test_unit_graph(cli)?;
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
        members_filter.as_deref(),
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

    // Auto-check override coverage after generation (unless suppressed)
    if !cli.no_check {
        let report = overrides::check_overrides(&plan);
        if report.total > 0 {
            eprintln!();
            eprintln!("Override coverage:");
            print_override_report(&report, false);
        }
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

    let report = overrides::check_overrides(&plan);
    print_override_report(&report, cli.json);
    Ok(())
}
