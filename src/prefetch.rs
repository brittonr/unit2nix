use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use crate::output::{NixBuildPlan, NixSource};

/// Prefetch result from `nix-prefetch-git` (only the fields we need).
#[derive(Debug, Deserialize)]
struct PrefetchGitResult {
    sha256: String,
}

/// Load crate-hashes.json from the workspace root and pre-fill sha256 values
/// for git sources in the build plan.
///
/// crate-hashes.json keys have the format:
///   `{url}?rev={rev}#{crate}@{version}` or `{url}#{crate}@{version}`
///
/// The sha256 values are SRI hashes (e.g. `sha256-xxxx=`), which `pkgs.fetchgit`
/// accepts directly.
///
/// This avoids calling `nix-prefetch-git` inside sandboxed derivations where
/// network access is unavailable.
pub fn apply_crate_hashes(plan: &mut NixBuildPlan, manifest_path: &Path) -> Result<()> {
    let hashes_path = manifest_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("crate-hashes.json");

    if !hashes_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&hashes_path)
        .with_context(|| format!("failed to read {}", hashes_path.display()))?;

    let hashes: BTreeMap<String, String> = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", hashes_path.display()))?;

    if hashes.is_empty() {
        return Ok(());
    }

    // Index hashes by (url, rev) for fast lookup.
    // Multiple crates from the same repo share the same hash.
    // Also index by url alone for entries without ?rev= in the key.
    let mut by_url_rev: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut by_url_only: BTreeMap<String, String> = BTreeMap::new();
    for (key, hash) in &hashes {
        if let Some((url, rev)) = parse_crate_hash_key(key) {
            match rev {
                Some(r) => { by_url_rev.entry((url, r)).or_insert_with(|| hash.clone()); }
                None => { by_url_only.entry(url).or_insert_with(|| hash.clone()); }
            }
        }
    }

    let mut applied = 0u32;
    for crate_info in plan.crates.values_mut() {
        if let Some(NixSource::Git {
            url,
            rev,
            sha256: sha256 @ None,
            ..
        }) = &mut crate_info.source
        {
            let hash = by_url_rev.get(&(url.clone(), rev.clone()))
                .or_else(|| by_url_only.get(url.as_str()));
            if let Some(hash) = hash {
                *sha256 = Some(hash.clone());
                applied += 1;
            }
        }
    }

    if applied > 0 {
        eprintln!("Applied {applied} git source hash(es) from crate-hashes.json");
    }

    Ok(())
}

/// Parse a crate-hashes.json key into (url, optional rev).
///
/// Key formats:
///   `https://example.com/repo.git?rev=abc123#crate@1.0.0`
///   `https://example.com/repo.git#crate@1.0.0` (no rev in URL)
fn parse_crate_hash_key(key: &str) -> Option<(String, Option<String>)> {
    // Split on '#' — left side is URL (possibly with ?rev=), right is crate info
    let url_part = key.split('#').next()?;

    // Extract URL base and rev from query params
    if let Some(query_start) = url_part.find('?') {
        let base = &url_part[..query_start];
        let query = &url_part[query_start + 1..];
        let rev = query
            .split('&')
            .find_map(|param| param.strip_prefix("rev="));
        Some((base.to_string(), rev.map(String::from)))
    } else {
        // No rev in URL — still usable, will match by URL alone
        Some((url_part.to_string(), None))
    }
}

/// Run `nix-prefetch-git` to get the SHA256 of a git checkout.
///
/// This produces a fixed-output hash that `pkgs.fetchgit` can use,
/// enabling pure flake evaluation without `--impure`.
///
/// # Errors
/// Returns an error if `nix-prefetch-git` is not found, fails, or
/// produces unparseable output.
pub fn prefetch_git(url: &str, rev: &str) -> Result<String> {
    let output = match Command::new("nix-prefetch-git")
        .args(["--url", url, "--rev", rev, "--fetch-submodules", "--leave-dotGit", "--quiet"])
        .output()
    {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            bail!(
                "nix-prefetch-git not found on PATH.\n\
                 Install it with `nix-env -iA nixpkgs.nix-prefetch-git`,\n\
                 or install via `nix profile install github:NixOS/nixpkgs#nix-prefetch-git`."
            );
        }
        Err(e) => {
            return Err(anyhow::Error::new(e)
                .context("failed to run nix-prefetch-git"));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "nix-prefetch-git failed for {url} at {rev}:\n{stderr}\n\n\
             hint: check that the URL is reachable: `git ls-remote {url}`"
        );
    }

    let result: PrefetchGitResult = serde_json::from_slice(&output.stdout)
        .context("failed to parse nix-prefetch-git JSON output")?;

    Ok(result.sha256)
}

/// Prefetch all git sources in the build plan, filling in their sha256 fields.
///
/// Deduplicates by (url, rev) so each git repo is fetched at most once,
/// even if multiple crates come from the same repo (monorepo deps).
///
/// # Errors
/// Returns an error if any git source fails to prefetch.
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
            let crate_info = plan
                .crates
                .get_mut(pkg_id)
                .ok_or_else(|| anyhow::anyhow!("internal error: pkg_id {pkg_id} missing from build plan during prefetch"))?;
            if let Some(NixSource::Git { sha256: ref mut hash, .. }) = &mut crate_info.source {
                *hash = Some(sha256.clone());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_key_with_rev() {
        let key = "https://git.snix.dev/snix/snix.git?rev=180bfc4ce41a#nix-compat@0.1.0";
        let (url, rev) = parse_crate_hash_key(key).unwrap();
        assert_eq!(url, "https://git.snix.dev/snix/snix.git");
        assert_eq!(rev, Some("180bfc4ce41a".to_string()));
    }

    #[test]
    fn parse_key_with_full_rev() {
        let key = "https://git.snix.dev/snix/snix.git?rev=180bfc4ce41ad25016aae2e3eb4e7af8c3d185ac#nix-compat@0.1.0";
        let (url, rev) = parse_crate_hash_key(key).unwrap();
        assert_eq!(url, "https://git.snix.dev/snix/snix.git");
        assert_eq!(rev, Some("180bfc4ce41ad25016aae2e3eb4e7af8c3d185ac".to_string()));
    }

    #[test]
    fn parse_key_no_rev_returns_url_only() {
        let key = "https://github.com/n0-computer/iroh-experiments#h3-iroh@0.1.0";
        let (url, rev) = parse_crate_hash_key(key).unwrap();
        assert_eq!(url, "https://github.com/n0-computer/iroh-experiments");
        assert_eq!(rev, None);
    }

    #[test]
    fn parse_key_github_with_rev() {
        let key = "https://github.com/s2-streamstore/mad-turmoil?rev=ef75169#mad-turmoil@0.2.0";
        let (url, rev) = parse_crate_hash_key(key).unwrap();
        assert_eq!(url, "https://github.com/s2-streamstore/mad-turmoil");
        assert_eq!(rev, Some("ef75169".to_string()));
    }
}
