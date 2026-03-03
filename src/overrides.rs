//! Known -sys crate registry for `--check-overrides`.
//!
//! Maps crate names to human-readable notes about what native libraries they need.
//! This is a compiled-in registry — it does not need to match the Nix-side
//! `lib/crate-overrides.nix` exactly, but should cover the same common cases.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::output::NixBuildPlan;

/// A known crate entry: what it needs and whether it's covered.
#[derive(Debug)]
pub struct KnownCrate {
    /// Human-readable note about what the crate needs.
    pub note: &'static str,
    /// Whether it's covered by nixpkgs defaultCrateOverrides or unit2nix built-ins.
    pub covered: bool,
}

/// Override status for a single crate in the report.
#[derive(Debug, Serialize)]
pub struct CrateOverrideStatus {
    pub name: String,
    pub links: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Aggregate override coverage report.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverrideReport {
    pub total: usize,
    pub covered: usize,
    pub no_override_needed: usize,
    pub missing: usize,
    pub crates: Vec<CrateOverrideStatus>,
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

/// Analyze override coverage for a build plan and return a structured report.
pub fn check_overrides(plan: &NixBuildPlan) -> OverrideReport {
    let registry = known_crates();

    let mut with_links: Vec<(&str, &str)> = Vec::new();
    for crate_info in plan.crates.values() {
        if let Some(ref links) = crate_info.links {
            with_links.push((&crate_info.crate_name, links));
        }
    }

    with_links.sort_by_key(|(name, _)| *name);

    let mut covered_count = 0;
    let mut no_override_count = 0;
    let mut missing_count = 0;
    let mut crate_statuses = Vec::new();

    for (crate_name, links_value) in &with_links {
        if is_known_no_override(crate_name, links_value) {
            no_override_count += 1;
            crate_statuses.push(CrateOverrideStatus {
                name: (*crate_name).to_string(),
                links: (*links_value).to_string(),
                status: "no-override-needed",
                note: Some("Rust-internal".to_string()),
            });
        } else if let Some(known) = registry.get(crate_name) {
            if known.covered {
                covered_count += 1;
                crate_statuses.push(CrateOverrideStatus {
                    name: (*crate_name).to_string(),
                    links: (*links_value).to_string(),
                    status: "covered",
                    note: Some(known.note.to_string()),
                });
            } else {
                missing_count += 1;
                crate_statuses.push(CrateOverrideStatus {
                    name: (*crate_name).to_string(),
                    links: (*links_value).to_string(),
                    status: "unknown",
                    note: Some(known.note.to_string()),
                });
            }
        } else {
            missing_count += 1;
            crate_statuses.push(CrateOverrideStatus {
                name: (*crate_name).to_string(),
                links: (*links_value).to_string(),
                status: "unknown",
                note: None,
            });
        }
    }

    OverrideReport {
        total: with_links.len(),
        covered: covered_count,
        no_override_needed: no_override_count,
        missing: missing_count,
        crates: crate_statuses,
    }
}

/// Print an override report in human-readable or JSON format.
pub fn print_override_report(report: &OverrideReport, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(report).expect("failed to serialize override report")
        );
        return;
    }

    if report.total == 0 {
        println!("No crates with native link requirements found.");
        println!("This is a pure Rust project — no overrides needed.");
        return;
    }

    println!(
        "Found {} crate(s) with native link requirements:\n",
        report.total
    );

    for cs in &report.crates {
        let icon = match cs.status {
            "covered" | "no-override-needed" => "✓",
            _ => "?",
        };
        let detail = cs
            .note
            .as_deref()
            .map_or(String::new(), |n| format!(" ({n})"));
        let status_label = match cs.status {
            "covered" => "covered",
            "no-override-needed" => "no override needed",
            _ => "may need extraCrateOverrides",
        };
        println!(
            "  {icon} {:30}  links={:20}  {status_label}{detail}",
            cs.name, cs.links
        );
    }

    println!();
    println!(
        "Summary: {} covered, {} no-override-needed, {} may need attention",
        report.covered, report.no_override_needed, report.missing
    );
    if report.missing > 0 {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{NixBuildPlan, NixCrate};
    use std::collections::BTreeMap;

    fn make_plan(crates: Vec<(&str, Option<&str>)>) -> NixBuildPlan {
        let mut plan_crates = BTreeMap::new();
        for (name, links) in crates {
            let pkg_id = format!("registry+https://github.com/rust-lang/crates.io-index#{name}@1.0.0");
            plan_crates.insert(
                pkg_id,
                NixCrate {
                    crate_name: name.to_string(),
                    version: "1.0.0".to_string(),
                    edition: "2021".to_string(),
                    sha256: None,
                    source: None,
                    features: vec![],
                    dependencies: vec![],
                    build_dependencies: vec![],
                    dev_dependencies: vec![],
                    proc_macro: false,
                    build: None,
                    lib_path: None,
                    lib_name: None,
                    lib_crate_types: vec![],
                    crate_bin: vec![],
                    links: links.map(str::to_owned),
                    authors: vec![],
                    description: None,
                    homepage: None,
                    license: None,
                    repository: None,
                },
            );
        }
        NixBuildPlan {
            version: 1,
            workspace_root: "/workspace".to_string(),
            roots: vec![],
            workspace_members: BTreeMap::new(),
            target: None,
            cargo_lock_hash: String::new(),
            crates: plan_crates,
        }
    }

    #[test]
    fn report_covered_and_unknown() {
        let plan = make_plan(vec![
            ("ring", Some("ring_core_0_17_14_")),
            ("my-custom-sys", Some("my_custom")),
            ("serde", None),
        ]);
        let report = check_overrides(&plan);
        assert_eq!(report.total, 2, "only crates with links");
        assert_eq!(report.no_override_needed, 1, "ring_core_ prefix is known-no-override");
        assert_eq!(report.missing, 1, "my-custom-sys is unknown");
        assert_eq!(report.covered, 0);
    }

    #[test]
    fn report_pure_rust() {
        let plan = make_plan(vec![
            ("serde", None),
            ("tokio", None),
        ]);
        let report = check_overrides(&plan);
        assert_eq!(report.total, 0);
        assert_eq!(report.missing, 0);
        assert!(report.crates.is_empty());
    }

    #[test]
    fn report_json_roundtrip() {
        let plan = make_plan(vec![
            ("ring", Some("ring_core_0_17_14_")),
            ("libz-sys", Some("z")),
        ]);
        let report = check_overrides(&plan);
        let json = serde_json::to_string(&report).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed["total"], 2);
        assert!(parsed["crates"].is_array());
        assert_eq!(parsed["crates"].as_array().unwrap().len(), 2);
    }
}
