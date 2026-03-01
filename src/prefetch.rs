use std::collections::BTreeMap;
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::output::{NixBuildPlan, NixSource};

/// Prefetch result from `nix-prefetch-git` (only the fields we need).
#[derive(Debug, Deserialize)]
struct PrefetchGitResult {
    sha256: String,
}

/// Run `nix-prefetch-git` to get the SHA256 of a git checkout.
///
/// This produces a fixed-output hash that `pkgs.fetchgit` can use,
/// enabling pure flake evaluation without `--impure`.
pub fn prefetch_git(url: &str, rev: &str) -> Result<String> {
    let output = Command::new("nix-prefetch-git")
        .args(["--url", url, "--rev", rev, "--fetch-submodules", "--quiet"])
        .output()
        .context("failed to run nix-prefetch-git (is it installed?)")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("nix-prefetch-git failed for {url} at {rev}:\n{stderr}");
    }

    let result: PrefetchGitResult = serde_json::from_slice(&output.stdout)
        .context("failed to parse nix-prefetch-git JSON output")?;

    Ok(result.sha256)
}

/// Prefetch all git sources in the build plan, filling in their sha256 fields.
///
/// Deduplicates by (url, rev) so each git repo is fetched at most once,
/// even if multiple crates come from the same repo (monorepo deps).
pub fn prefetch_git_sources(plan: &mut NixBuildPlan) -> Result<()> {
    // Collect unique (url, rev) pairs that need prefetching
    let mut to_prefetch: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    for (pkg_id, crate_info) in &plan.crates {
        if let Some(NixSource::Git { url, rev, sha256: None, .. }) = &crate_info.source {
            to_prefetch
                .entry((url.clone(), rev.clone()))
                .or_default()
                .push(pkg_id.clone());
        }
    }

    if to_prefetch.is_empty() {
        return Ok(());
    }

    let total = to_prefetch.len();
    eprintln!("Prefetching {total} git source(s)...");

    for (idx, ((url, rev), pkg_ids)) in to_prefetch.iter().enumerate() {
        let short_rev = if rev.len() > 12 { &rev[..12] } else { rev };
        eprintln!("  [{}/{}] {} @ {}", idx + 1, total, url, short_rev);

        let sha256 = prefetch_git(url, rev)?;

        // Apply the hash to all crates from this repo
        for pkg_id in pkg_ids {
            if let Some(NixSource::Git { sha256: ref mut hash, .. }) = &mut plan.crates.get_mut(pkg_id)
                .expect("pkg_id was collected from this map").source {
                *hash = Some(sha256.clone());
            }
        }
    }

    Ok(())
}
