//! Known -sys crate registry for `--check-overrides`.
//!
//! Maps crate names to human-readable notes about what native libraries they need.
//! This is a compiled-in registry — it does not need to match the Nix-side
//! `lib/crate-overrides.nix` exactly, but should cover the same common cases.

use std::collections::BTreeMap;

use crate::output::NixBuildPlan;

/// A known crate entry: what it needs and whether it's covered.
#[derive(Debug)]
pub struct KnownCrate {
    /// Human-readable note about what the crate needs.
    pub note: &'static str,
    /// Whether it's covered by nixpkgs defaultCrateOverrides or unit2nix built-ins.
    pub covered: bool,
}

/// Crates whose `links` field is Rust-internal and never need native overrides.
fn known_no_override() -> &'static [&'static str] {
    &[
        "rayon-core",
        "prettyplease",
        "compiler_builtins",
        "rustc-std-workspace-core",
        "rustc-std-workspace-alloc",
    ]
}

/// Prefix patterns for links values that are Rust-internal.
fn known_no_override_prefixes() -> &'static [&'static str] {
    &["ring_core_"]
}

fn is_known_no_override(crate_name: &str, links_value: &str) -> bool {
    known_no_override().contains(&crate_name)
        || known_no_override_prefixes()
            .iter()
            .any(|prefix| links_value.starts_with(prefix))
}

/// Build the known-crate registry.
fn known_crates() -> BTreeMap<&'static str, KnownCrate> {
    let mut m = BTreeMap::new();

    // Covered by nixpkgs defaultCrateOverrides
    let nixpkgs = [
        ("openssl-sys", "needs pkg-config + openssl.dev"),
        ("libgit2-sys", "needs pkg-config + libgit2"),
        ("libz-sys", "needs pkg-config + zlib"),
        ("libsqlite3-sys", "needs pkg-config + sqlite.dev"),
        ("libssh2-sys", "needs pkg-config + libssh2"),
        ("curl-sys", "needs pkg-config + curl"),
        ("libdbus-sys", "needs pkg-config + dbus"),
        ("prost-build", "needs protobuf compiler"),
        ("rdkafka-sys", "needs pkg-config + rdkafka"),
        ("pq-sys", "needs pkg-config + postgresql"),
        ("alsa-sys", "needs pkg-config + alsa-lib"),
        ("freetype-sys", "needs pkg-config + freetype"),
        ("expat-sys", "needs pkg-config + expat"),
        ("libudev-sys", "needs pkg-config + udev"),
        ("aws-lc-sys", "needs cmake + go"),
    ];
    for (name, note) in nixpkgs {
        m.insert(name, KnownCrate { note, covered: true });
    }

    // Covered by unit2nix built-in overrides (lib/crate-overrides.nix)
    let unit2nix = [
        ("ring", "needs perl for build script assembly compilation"),
        ("tikv-jemalloc-sys", "needs make for vendored jemalloc build"),
        ("jemalloc-sys", "needs make for vendored jemalloc build"),
        ("onig_sys", "needs pkg-config + oniguruma"),
        ("librocksdb-sys", "needs cmake + rocksdb"),
        ("zstd-sys", "needs pkg-config + zstd"),
        ("bzip2-sys", "needs pkg-config + bzip2"),
        ("lzma-sys", "needs pkg-config + xz/lzma"),
    ];
    for (name, note) in unit2nix {
        m.insert(name, KnownCrate { note, covered: true });
    }

    m
}

/// Check override coverage for a build plan and print a report.
pub fn check_overrides(plan: &NixBuildPlan) {
    let registry = known_crates();

    // Collect crates with links fields
    let mut with_links: Vec<(&str, &str)> = Vec::new();
    for crate_info in plan.crates.values() {
        if let Some(ref links) = crate_info.links {
            with_links.push((&crate_info.crate_name, links));
        }
    }

    if with_links.is_empty() {
        println!("No crates with native link requirements found.");
        println!("This is a pure Rust project — no overrides needed.");
        return;
    }

    with_links.sort_by_key(|(name, _)| *name);

    println!(
        "Found {} crate(s) with native link requirements:\n",
        with_links.len()
    );

    let mut covered_count = 0;
    let mut no_override_count = 0;
    let mut missing_count = 0;

    for (crate_name, links_value) in &with_links {
        if is_known_no_override(crate_name, links_value) {
            println!("  ✓ {crate_name:30}  links={links_value:20}  (no override needed — Rust-internal)");
            no_override_count += 1;
        } else if let Some(known) = registry.get(crate_name) {
            if known.covered {
                println!("  ✓ {crate_name:30}  links={links_value:20}  (covered — {})", known.note);
                covered_count += 1;
            } else {
                println!("  ✗ {crate_name:30}  links={links_value:20}  (known but not auto-covered — {})", known.note);
                missing_count += 1;
            }
        } else {
            println!("  ? {crate_name:30}  links={links_value:20}  (unknown — may need extraCrateOverrides)");
            missing_count += 1;
        }
    }

    println!();
    println!("Summary: {covered_count} covered, {no_override_count} no-override-needed, {missing_count} may need attention");
    if missing_count > 0 {
        println!();
        println!("For missing crates, add overrides via extraCrateOverrides:");
        println!("  buildFromUnitGraph {{");
        println!("    extraCrateOverrides = {{");
        println!("      <crate-name> = attrs: {{");
        println!("        nativeBuildInputs = [ pkgs.pkg-config ];");
        println!("        buildInputs = [ pkgs.<library> ];");
        println!("      }};");
        println!("    }};");
        println!("  }};");
        println!();
        println!("See docs/sys-crate-overrides.md for details.");
    }
}
