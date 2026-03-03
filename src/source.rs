use anyhow::{bail, Result};

use crate::output::NixSource;

/// Parsed components of a `git+URL?query#fragment` string.
struct ParsedGitUrl {
    /// Base URL (everything before `?` or `#`).
    url: String,
    /// Value of the `?rev=` query parameter, if present.
    rev_query: Option<String>,
    /// The fragment after `#` (commit hash in source strings, `name@version` in `pkg_ids`).
    fragment: String,
}

/// Parse a `git+URL?query#fragment` string into its components.
///
/// The interpretation of the fragment depends on context:
/// - In metadata source strings (`parse_source`): the fragment is the full commit hash.
/// - In package IDs (`infer_source_from_pkg_id`): the fragment is `name@version`.
fn parse_git_url(s: &str) -> Option<ParsedGitUrl> {
    let without_prefix = s.strip_prefix("git+")?;
    let (url_part, fragment) = without_prefix.rsplit_once('#')?;
    let (base_url, query) = url_part.split_once('?').unwrap_or((url_part, ""));

    let rev_query = query
        .split('&')
        .find_map(|param| param.strip_prefix("rev="))
        .map(str::to_owned);

    Some(ParsedGitUrl {
        url: base_url.to_string(),
        rev_query,
        fragment: fragment.to_string(),
    })
}

/// Parse source string from cargo metadata into a [`NixSource`].
///
/// For git dependencies, computes `sub_dir` from the `manifest_path` relative
/// to Cargo's git checkout cache. This handles monorepo git deps where the
/// crate lives in a subdirectory (e.g., `{ git = "...", subdirectory = "crates/foo" }`).
/// Returns:
/// - `Ok(Some(...))` — known source type resolved successfully.
/// - `Ok(None)` — source field is `None` in metadata but `manifest_path` is empty
///   (shouldn't happen in practice; local deps always have a `manifest_path`).
/// - `Err(...)` — unknown or malformed source type. Callers should fall back to
///   `infer_source_from_pkg_id` or propagate the error.
pub fn parse_source(source: Option<&str>, manifest_path: &str, workspace_root: &str) -> Result<Option<NixSource>> {
    match source {
        None => {
            // Local path dependency — compute relative path from workspace root
            let manifest = std::path::Path::new(manifest_path);
            let crate_dir = manifest.parent().unwrap_or(std::path::Path::new("."));
            let ws = std::path::Path::new(workspace_root);
            let rel = crate_dir.strip_prefix(ws).map_or_else(
                |_| crate_dir.to_string_lossy().into_owned(),
                |p| p.to_string_lossy().into_owned(),
            );
            let path = if rel.is_empty() { ".".to_string() } else { rel };
            Ok(Some(NixSource::Local { path }))
        }
        Some(s) if s.starts_with("registry+") => {
            // Extract registry URL for non-crates.io registries
            let registry_url = s.strip_prefix("registry+").unwrap_or("");
            if registry_url == "https://github.com/rust-lang/crates.io-index" {
                Ok(Some(NixSource::CratesIo))
            } else {
                Ok(Some(NixSource::Registry {
                    index: registry_url.to_string(),
                }))
            }
        }
        Some(s) if s.starts_with("git+") => {
            let parsed = parse_git_url(s)
                .ok_or_else(|| anyhow::anyhow!("malformed git source URL: {s}"))?;
            // In metadata source strings, the fragment is always the full commit hash.
            let rev = parsed.fragment;
            let sub_dir = compute_git_subdir(manifest_path);
            Ok(Some(NixSource::Git { url: parsed.url, rev, sub_dir, sha256: None }))
        }
        Some(s) => {
            bail!("unknown source type: {s}")
        }
    }
}

/// Compute the subdirectory of a crate within a git checkout.
///
/// Cargo stores git checkouts at `~/.cargo/git/checkouts/<name>/<hash>/`.
/// If the crate's Cargo.toml is at `.../checkouts/<name>/<hash>/sub/path/Cargo.toml`,
/// this returns `Some("sub/path")`. Returns `None` if the crate is at the repo root.
pub fn compute_git_subdir(manifest_path: &str) -> Option<String> {
    // Look for the checkouts/<name>/<hash>/ pattern
    let parts: Vec<&str> = manifest_path.split('/').collect();
    let checkout_idx = parts.iter().position(|&p| p == "checkouts")?;

    // checkouts/<name>/<hash>/ is 3 components after "checkouts"
    let root_idx = checkout_idx + 3;
    if root_idx >= parts.len() {
        return None;
    }

    // Everything between the checkout root and "Cargo.toml" is the subdirectory
    let end_idx = parts.len() - 1; // skip "Cargo.toml"
    if parts[end_idx] != "Cargo.toml" {
        return None;
    }

    if end_idx <= root_idx {
        return None; // Crate is at repo root
    }

    let sub = parts[root_idx..end_idx].join("/");
    if sub.is_empty() { None } else { Some(sub) }
}

/// Infer source type from a package ID when metadata is missing.
///
/// Some crates appear in the unit graph but not in `cargo metadata` (e.g.,
/// transitive deps pulled in by feature-specific resolution that metadata's
/// resolver doesn't include). The `pkg_id` format encodes the source:
/// - `registry+https://...#name@version` → crates.io or alternative registry
/// - `path+file:///...#version` → local
/// - `git+https://...?rev=HASH#name@version` → git (rev from query param)
/// - `git+https://...#HASH` → git (rev from fragment)
pub fn infer_source_from_pkg_id(pkg_id: &str) -> Option<NixSource> {
    if pkg_id.starts_with("registry+https://github.com/rust-lang/crates.io-index") {
        Some(NixSource::CratesIo)
    } else if pkg_id.starts_with("registry+") {
        let registry_url = pkg_id
            .strip_prefix("registry+")?
            .split('#')
            .next()?;
        Some(NixSource::Registry {
            index: registry_url.to_string(),
        })
    } else if pkg_id.starts_with("git+") {
        let parsed = parse_git_url(pkg_id)?;
        // In pkg_ids, the fragment is `name@version` (NOT a git rev).
        // The actual rev comes from the `?rev=` query parameter.
        // Fall back to the fragment only if it looks like a bare hash (no `@`).
        let rev = if let Some(rev) = parsed.rev_query {
            rev
        } else if !parsed.fragment.contains('@') {
            // Fragment is a bare commit hash (no name@ prefix)
            parsed.fragment
        } else {
            eprintln!(
                "warning: git dep has no ?rev= and fragment is name@version, \
                 cannot determine rev: {pkg_id}"
            );
            return None;
        };
        Some(NixSource::Git {
            url: parsed.url,
            rev,
            sub_dir: None,
            sha256: None,
        })
    } else {
        // path+ or unknown — local
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_source_crates_io() {
        let source = parse_source(
            Some("registry+https://github.com/rust-lang/crates.io-index"),
            "",
            "",
        ).unwrap();
        assert!(matches!(source, Some(NixSource::CratesIo)));
    }

    #[test]
    fn parse_source_alternative_registry() {
        let source = parse_source(
            Some("registry+https://dl.cloudsmith.io/public/my-org/my-repo/cargo/index.git"),
            "",
            "",
        ).unwrap();
        match source {
            Some(NixSource::Registry { index }) => {
                assert_eq!(index, "https://dl.cloudsmith.io/public/my-org/my-repo/cargo/index.git");
            }
            other => panic!("expected Registry, got {other:?}"),
        }
    }

    #[test]
    fn parse_source_local() {
        let source = parse_source(None, "/home/user/project/crates/foo/Cargo.toml", "/home/user/project").unwrap();
        match source {
            Some(NixSource::Local { path }) => assert_eq!(path, "crates/foo"),
            other => panic!("expected Local, got {other:?}"),
        }
    }

    #[test]
    fn parse_source_local_root() {
        let source = parse_source(None, "/home/user/project/Cargo.toml", "/home/user/project").unwrap();
        match source {
            Some(NixSource::Local { path }) => assert_eq!(path, "."),
            other => panic!("expected Local with '.', got {other:?}"),
        }
    }

    #[test]
    fn parse_source_git() {
        let source = parse_source(
            Some("git+https://github.com/example/repo.git?rev=abc123#abc123def456"),
            "/home/user/.cargo/git/checkouts/repo/abc123/Cargo.toml",
            "",
        ).unwrap();
        match source {
            Some(NixSource::Git { url, rev, sub_dir, sha256 }) => {
                assert_eq!(url, "https://github.com/example/repo.git");
                assert_eq!(rev, "abc123def456", "should use fragment (full hash) as rev");
                assert_eq!(sub_dir, None, "root-level crate should have no sub_dir");
                assert_eq!(sha256, None, "sha256 is filled later by prefetch");
            }
            other => panic!("expected Git, got {other:?}"),
        }
    }

    #[test]
    fn parse_source_git_subdir() {
        let source = parse_source(
            Some("git+https://github.com/org/monorepo.git?rev=abc123#abc123def456"),
            "/home/user/.cargo/git/checkouts/monorepo/abc123/crates/my-crate/Cargo.toml",
            "",
        ).unwrap();
        match source {
            Some(NixSource::Git { url, rev, sub_dir, sha256 }) => {
                assert_eq!(url, "https://github.com/org/monorepo.git");
                assert_eq!(rev, "abc123def456");
                assert_eq!(sub_dir, Some("crates/my-crate".to_string()));
                assert_eq!(sha256, None, "sha256 is filled later by prefetch");
            }
            other => panic!("expected Git with sub_dir, got {other:?}"),
        }
    }

    #[test]
    fn parse_source_unknown_type_errors() {
        let result = parse_source(Some("sparse+https://example.com/index/"), "", "");
        assert!(result.is_err(), "unknown source type should return Err");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("unknown source type"), "error should mention unknown type: {msg}");
    }

    #[test]
    fn parse_source_malformed_git_errors() {
        // Missing fragment (no #)
        let result = parse_source(Some("git+https://example.com/repo.git"), "", "");
        assert!(result.is_err(), "malformed git URL should return Err");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("malformed"), "error should mention malformed: {msg}");
    }

    #[test]
    fn compute_git_subdir_root() {
        assert_eq!(
            compute_git_subdir("/home/user/.cargo/git/checkouts/repo/abc123/Cargo.toml"),
            None
        );
    }

    #[test]
    fn compute_git_subdir_nested() {
        assert_eq!(
            compute_git_subdir("/home/user/.cargo/git/checkouts/repo/abc123/sub/path/Cargo.toml"),
            Some("sub/path".to_string())
        );
    }

    #[test]
    fn compute_git_subdir_no_checkouts() {
        assert_eq!(
            compute_git_subdir("/some/random/path/Cargo.toml"),
            None
        );
    }

    #[test]
    fn infer_git_with_rev_query_param() {
        // pkg_id format: git+URL?rev=HASH#name@version
        // The rev should come from ?rev=, NOT the fragment's version.
        let source = infer_source_from_pkg_id(
            "git+https://github.com/example/repo.git?rev=abc123def456#my-crate@0.5.0",
        );
        match source {
            Some(NixSource::Git { url, rev, .. }) => {
                assert_eq!(url, "https://github.com/example/repo.git");
                assert_eq!(rev, "abc123def456", "should use ?rev= query param, not fragment version");
            }
            other => panic!("expected Git, got {other:?}"),
        }
    }

    #[test]
    fn infer_git_with_bare_hash() {
        // When fragment is a bare hash (no @), use it directly
        let source = infer_source_from_pkg_id(
            "git+https://github.com/example/repo.git#abc123def456",
        );
        match source {
            Some(NixSource::Git { url, rev, .. }) => {
                assert_eq!(url, "https://github.com/example/repo.git");
                assert_eq!(rev, "abc123def456");
            }
            other => panic!("expected Git, got {other:?}"),
        }
    }

    #[test]
    fn infer_git_no_rev_with_name_at_version() {
        // If no ?rev= and fragment is name@version, we can't determine the rev
        let source = infer_source_from_pkg_id(
            "git+https://github.com/example/repo.git#my-crate@0.5.0",
        );
        assert!(source.is_none(), "should return None when rev cannot be determined");
    }
}
