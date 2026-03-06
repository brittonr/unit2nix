use anyhow::{bail, Context, Result};

use crate::cargo;
use crate::cli::Cli;
use crate::fingerprint;
use crate::merge;
use crate::output::NixBuildPlan;
use crate::overrides::{self, print_override_report};
use crate::prefetch;

/// Shared entry point for both `unit2nix` and `cargo unit2nix`.
///
/// # Errors
/// Returns an error if cargo commands fail, merge produces invalid output,
/// or the output file cannot be written.
pub fn run(cli: &Cli) -> Result<()> {
    // --check-overrides: read an existing build plan and report coverage
    if cli.check_overrides {
        return run_check_overrides(cli);
    }

    // Validate: --members and --package are mutually exclusive
    if cli.members.is_some() && cli.package.is_some() {
        bail!("--members and --package cannot be used together");
    }

    // Compute inputs fingerprint for incremental skipping.
    // Skip the check when writing to stdout (user wants the output regardless).
    let inputs_hash = if cli.stdout {
        None
    } else {
        let hash = fingerprint::compute_inputs_hash(cli)?;
        if !cli.force {
            if let Some(existing) = fingerprint::read_existing_inputs_hash(&cli.output) {
                if existing == hash {
                    eprintln!(
                        "Build plan is up to date (use --force to regenerate)"
                    );
                    return Ok(());
                }
            }
        }
        Some(hash)
    };

    let members_filter = cli.members_filter();

    eprintln!("Running cargo build --unit-graph...");
    let unit_graph = cargo::run_unit_graph(cli)?;
    eprintln!("  {} units, {} roots", unit_graph.units.len(), unit_graph.roots.len());

    eprintln!("Running cargo metadata...");
    let metadata = cargo::run_cargo_metadata(cli)?;
    eprintln!("  {} packages", metadata.packages.len());

    eprintln!("Reading Cargo.lock...");
    let (lock, cargo_lock_hash) = cargo::read_cargo_lock(&cli.manifest_path)?;
    eprintln!(
        "  {} packages with checksums",
        lock.package
            .as_ref()
            .map_or(0, |p| p.iter().filter(|p| p.checksum.is_some()).count())
    );
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

    // Store the inputs fingerprint so the next run can skip if unchanged
    plan.inputs_hash = inputs_hash;

    let json = serde_json::to_string_pretty(&plan)?;

    if cli.stdout {
        println!("{json}");
    } else {
        std::fs::write(&cli.output, &json)
            .with_context(|| format!("failed to write output to {}", cli.output.display()))?;
        eprintln!("Wrote {}", cli.output.display());
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
    let path = &cli.output;

    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read build plan from {}", path.display()))?;

    let plan: NixBuildPlan = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse build plan from {}", path.display()))?;

    let report = overrides::check_overrides(&plan);
    print_override_report(&report, cli.json);
    Ok(())
}
