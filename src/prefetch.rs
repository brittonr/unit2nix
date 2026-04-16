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

    let mut failed = 0u32;
    for (idx, ((url, rev), pkg_ids)) in to_prefetch.iter().enumerate() {
        let short_rev = if rev.len() > 12 { &rev[..12] } else { rev };
        eprintln!("  [{}/{}] {} @ {}", idx + 1, total, url, short_rev);

        match prefetch_git(url, rev) {
            Ok(sha256) => {
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
            Err(e) => {
                failed += 1;
                let names: Vec<&str> = pkg_ids.iter()
                    .filter_map(|id| plan.crates.get(id).map(|c| c.crate_name.as_str()))
                    .collect();
                eprintln!(
                    "  ⚠ prefetch failed for {} ({}): {:#}\n    \
                     The Nix build will require --impure (builtins.fetchGit fallback).\n    \
                     To fix: push the commit to the remote, then regenerate the build plan.",
                    names.join(", "), short_rev, e
                );
            }
        }
    }

    if failed > 0 {
        eprintln!(
            "\n  {failed} git source(s) could not be prefetched.\n  \
             The build plan was written, but `nix build` will need --impure unless\n  \
             you push the missing commits and regenerate with `nix run .#update-plan`."
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{NixBuildPlan, NixCrate};
    use crate::test_support::env_lock;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn with_fake_prefetch<T>(script_body: Option<&str>, f: impl FnOnce() -> T) -> T {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        if let Some(body) = script_body {
            let path = dir.path().join("nix-prefetch-git");
            std::fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&path).unwrap().permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&path, perms).unwrap();
            }
        }

        let old_path = std::env::var_os("PATH");
        std::env::set_var("PATH", dir.path());
        let result = f();
        match old_path {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
        result
    }

    fn make_plan_with_git_crates(crates: Vec<(&str, &str, &str)>) -> NixBuildPlan {
        let mut plan_crates = BTreeMap::new();
        for (pkg_id, url, rev) in crates {
            let crate_info = NixCrate {
                crate_name: pkg_id.split('#').next().unwrap_or(pkg_id).to_string(),
                version: "0.1.0".to_string(),
                edition: "2021".to_string(),
                source: Some(NixSource::Git {
                    url: url.to_string(),
                    rev: rev.to_string(),
                    sub_dir: None,
                    sha256: None,
                }),
                ..NixCrate::default()
            };
            plan_crates.insert(pkg_id.to_string(), crate_info);
        }
        NixBuildPlan {
            version: 1,
            workspace_root: "/workspace".to_string(),
            roots: vec![],
            workspace_members: BTreeMap::new(),
            target: None,
            cargo_lock_hash: "hash".to_string(),
            inputs_hash: None,
            crates: plan_crates,
        }
    }

    fn write_hashes_fixture(dir: &tempfile::TempDir, content: &str) -> PathBuf {
        let manifest_path = dir.path().join("Cargo.toml");
        std::fs::write(dir.path().join("crate-hashes.json"), content).unwrap();
        std::fs::write(&manifest_path, "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n").unwrap();
        manifest_path
    }

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

    #[test]
    fn apply_crate_hashes_matches_exact_url_and_rev() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = write_hashes_fixture(
            &dir,
            r#"{
  "https://example.com/repo.git?rev=abc123#crate-a@0.1.0": "sha256-exact"
}"#,
        );
        let mut plan = make_plan_with_git_crates(vec![
            ("crate-a#0.1.0", "https://example.com/repo.git", "abc123"),
            ("crate-b#0.1.0", "https://example.com/repo.git", "def456"),
        ]);

        apply_crate_hashes(&mut plan, &manifest_path).unwrap();

        match &plan.crates["crate-a#0.1.0"].source {
            Some(NixSource::Git { sha256, .. }) => assert_eq!(sha256.as_deref(), Some("sha256-exact")),
            other => panic!("expected git source, got {other:?}"),
        }
        match &plan.crates["crate-b#0.1.0"].source {
            Some(NixSource::Git { sha256, .. }) => assert_eq!(sha256, &None),
            other => panic!("expected git source, got {other:?}"),
        }
    }

    #[test]
    fn apply_crate_hashes_matches_url_only_and_shares_across_multiple_crates() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = write_hashes_fixture(
            &dir,
            r#"{
  "https://example.com/monorepo.git#crate-a@0.1.0": "sha256-shared"
}"#,
        );
        let mut plan = make_plan_with_git_crates(vec![
            ("crate-a#0.1.0", "https://example.com/monorepo.git", "abc123"),
            ("crate-b#0.1.0", "https://example.com/monorepo.git", "def456"),
        ]);

        apply_crate_hashes(&mut plan, &manifest_path).unwrap();

        for pkg_id in ["crate-a#0.1.0", "crate-b#0.1.0"] {
            match &plan.crates[pkg_id].source {
                Some(NixSource::Git { sha256, .. }) => {
                    assert_eq!(sha256.as_deref(), Some("sha256-shared"), "pkg_id: {pkg_id}");
                }
                other => panic!("expected git source, got {other:?}"),
            }
        }
    }

    #[test]
    fn apply_crate_hashes_does_not_overwrite_existing_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let manifest_path = write_hashes_fixture(
            &dir,
            r#"{
  "https://example.com/repo.git?rev=abc123#crate-a@0.1.0": "sha256-new"
}"#,
        );
        let mut plan = make_plan_with_git_crates(vec![("crate-a#0.1.0", "https://example.com/repo.git", "abc123")]);
        if let Some(NixSource::Git { sha256, .. }) = &mut plan.crates.get_mut("crate-a#0.1.0").unwrap().source {
            *sha256 = Some("sha256-existing".to_string());
        }

        apply_crate_hashes(&mut plan, &manifest_path).unwrap();

        match &plan.crates["crate-a#0.1.0"].source {
            Some(NixSource::Git { sha256, .. }) => assert_eq!(sha256.as_deref(), Some("sha256-existing")),
            other => panic!("expected git source, got {other:?}"),
        }
    }

    #[test]
    fn prefetch_git_reports_missing_binary() {
        let err = with_fake_prefetch(None, || {
            prefetch_git("https://example.com/repo.git", "abc123")
                .unwrap_err()
                .to_string()
        });
        assert!(err.contains("nix-prefetch-git not found"), "got: {err}");
    }

    #[test]
    fn prefetch_git_reports_stderr_on_failure() {
        let err = with_fake_prefetch(Some("echo 'network exploded' >&2\nexit 1"), || {
            prefetch_git("https://example.com/repo.git", "deadbeef")
                .unwrap_err()
                .to_string()
        });
        assert!(err.contains("network exploded"), "got: {err}");
        assert!(err.contains("git ls-remote https://example.com/repo.git"), "got: {err}");
    }

    #[test]
    fn prefetch_git_parses_json_output() {
        let hash = with_fake_prefetch(
            Some("printf '{\"sha256\":\"sha256-fake\"}'"),
            || prefetch_git("https://example.com/repo.git", "abc123").unwrap(),
        );
        assert_eq!(hash, "sha256-fake");
    }

    #[test]
    fn prefetch_git_sources_deduplicates_by_url_and_rev() {
        let log_dir = tempfile::tempdir().unwrap();
        let log_path = log_dir.path().join("prefetch.log");
        let script = format!(
            "printf '%s\\n' \"$*\" >> '{}'\nprintf '{{\"sha256\":\"sha256-fake\"}}'",
            log_path.display()
        );
        let mut plan = make_plan_with_git_crates(vec![
            ("crate-a#0.1.0", "https://example.com/repo.git", "abc123"),
            ("crate-b#0.1.0", "https://example.com/repo.git", "abc123"),
            ("crate-c#0.1.0", "https://example.com/repo.git", "def456"),
        ]);

        with_fake_prefetch(Some(&script), || prefetch_git_sources(&mut plan).unwrap());

        let log = std::fs::read_to_string(&log_path).unwrap();
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(lines.len(), 2, "expected one prefetch per unique (url, rev), got: {lines:?}");
        for pkg_id in ["crate-a#0.1.0", "crate-b#0.1.0", "crate-c#0.1.0"] {
            match &plan.crates[pkg_id].source {
                Some(NixSource::Git { sha256, .. }) => {
                    assert_eq!(sha256.as_deref(), Some("sha256-fake"), "pkg_id: {pkg_id}");
                }
                other => panic!("expected git source, got {other:?}"),
            }
        }
    }

    #[test]
    fn prefetch_git_sources_continues_after_individual_failures() {
        let script = "
rev=''
while [ $# -gt 0 ]; do
  if [ \"$1\" = '--rev' ]; then
    shift
    rev=\"$1\"
    break
  fi
  shift
done
if [ \"$rev\" = 'badrev' ]; then
  echo 'cannot prefetch badrev' >&2
  exit 1
fi
printf '{\"sha256\":\"sha256-good\"}'
";
        let mut plan = make_plan_with_git_crates(vec![
            ("crate-good#0.1.0", "https://example.com/repo.git", "goodrev"),
            ("crate-bad#0.1.0", "https://example.com/repo.git", "badrev"),
        ]);

        with_fake_prefetch(Some(script), || prefetch_git_sources(&mut plan).unwrap());

        match &plan.crates["crate-good#0.1.0"].source {
            Some(NixSource::Git { sha256, .. }) => assert_eq!(sha256.as_deref(), Some("sha256-good")),
            other => panic!("expected git source, got {other:?}"),
        }
        match &plan.crates["crate-bad#0.1.0"].source {
            Some(NixSource::Git { sha256, .. }) => assert_eq!(sha256, &None),
            other => panic!("expected git source, got {other:?}"),
        }
    }

    #[test]
    fn prefetch_git_sources_skips_crates_with_existing_hashes() {
        let log_dir = tempfile::tempdir().unwrap();
        let log_path = log_dir.path().join("prefetch.log");
        let script = format!(
            "printf '%s\\n' \"$*\" >> '{}'\nprintf '{{\"sha256\":\"sha256-fake\"}}'",
            log_path.display()
        );
        let mut plan = make_plan_with_git_crates(vec![
            ("crate-hashed#0.1.0", "https://example.com/repo.git", "abc123"),
            ("crate-missing#0.1.0", "https://example.com/repo.git", "def456"),
        ]);
        if let Some(NixSource::Git { sha256, .. }) = &mut plan.crates.get_mut("crate-hashed#0.1.0").unwrap().source {
            *sha256 = Some("sha256-existing".to_string());
        }

        with_fake_prefetch(Some(&script), || prefetch_git_sources(&mut plan).unwrap());

        let log = std::fs::read_to_string(&log_path).unwrap();
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(lines.len(), 1, "only unhashed crate should be prefetched: {lines:?}");
        match &plan.crates["crate-hashed#0.1.0"].source {
            Some(NixSource::Git { sha256, .. }) => assert_eq!(sha256.as_deref(), Some("sha256-existing")),
            other => panic!("expected git source, got {other:?}"),
        }
        match &plan.crates["crate-missing#0.1.0"].source {
            Some(NixSource::Git { sha256, .. }) => assert_eq!(sha256.as_deref(), Some("sha256-fake")),
            other => panic!("expected git source, got {other:?}"),
        }
    }
}
