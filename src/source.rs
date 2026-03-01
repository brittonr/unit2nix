use crate::output::NixSource;

/// Parse a `git+URL?query#fragment` string into `(url, rev)`.
///
/// Handles two fragment formats:
/// - `git+URL#HASH` — the fragment is the commit rev
/// - `git+URL#name@version` — strip the `name@` prefix to get the rev
fn parse_git_url(s: &str) -> Option<(String, String)> {
    let without_prefix = s.strip_prefix("git+")?;
    let (url_part, fragment) = without_prefix.rsplit_once('#')?;
    // Fragment may be a bare rev hash or "name@version" — strip the name@ prefix
    let rev = fragment
        .rsplit_once('@')
        .map(|(_, v)| v)
        .unwrap_or(fragment);
    let url = url_part.split('?').next().unwrap_or(url_part);
    Some((url.to_string(), rev.to_string()))
}

/// Parse source string from cargo metadata into a NixSource.
///
/// For git dependencies, computes `sub_dir` from the manifest_path relative
/// to Cargo's git checkout cache. This handles monorepo git deps where the
/// crate lives in a subdirectory (e.g., `{ git = "...", subdirectory = "crates/foo" }`).
pub fn parse_source(source: Option<&str>, manifest_path: &str, workspace_root: &str) -> Option<NixSource> {
    match source {
        None => {
            // Local path dependency — compute relative path from workspace root
            let manifest = std::path::Path::new(manifest_path);
            let crate_dir = manifest.parent().unwrap_or(std::path::Path::new("."));
            let crate_dir_str = crate_dir.to_string_lossy().to_string();
            let ws = workspace_root.trim_end_matches('/');
            let rel = crate_dir_str
                .strip_prefix(ws)
                .map(|s| s.strip_prefix('/').unwrap_or(s))
                .unwrap_or(&crate_dir_str);
            let path = if rel.is_empty() { "." } else { rel };
            Some(NixSource::Local {
                path: path.to_string(),
            })
        }
        Some(s) if s.starts_with("registry+") => {
            // Extract registry URL for non-crates.io registries
            let registry_url = s.strip_prefix("registry+").unwrap_or("");
            if registry_url == "https://github.com/rust-lang/crates.io-index" {
                Some(NixSource::CratesIo)
            } else {
                Some(NixSource::Registry {
                    index: registry_url.to_string(),
                })
            }
        }
        Some(s) if s.starts_with("git+") => {
            let (url, rev) = parse_git_url(s)?;
            let sub_dir = compute_git_subdir(manifest_path);
            Some(NixSource::Git { url, rev, sub_dir, sha256: None })
        }
        Some(s) => {
            eprintln!("warning: unknown source type, treating as local: {s}");
            None
        }
        // None handled above
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
/// resolver doesn't include). The pkg_id format encodes the source:
/// - `registry+https://...#name@version` → crates.io or alternative registry
/// - `path+file:///...#version` → local
/// - `git+https://...#name@version` or `git+https://...#hash` → git
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
        let (url, rev) = parse_git_url(pkg_id)?;
        Some(NixSource::Git {
            url,
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
    use crate::output::NixSource;

    #[test]
    fn parse_source_crates_io() {
        let source = parse_source(
            Some("registry+https://github.com/rust-lang/crates.io-index"),
            "",
            "",
        );
        assert!(matches!(source, Some(NixSource::CratesIo)));
    }

    #[test]
    fn parse_source_alternative_registry() {
        let source = parse_source(
            Some("registry+https://dl.cloudsmith.io/public/my-org/my-repo/cargo/index.git"),
            "",
            "",
        );
        match source {
            Some(NixSource::Registry { index }) => {
                assert_eq!(index, "https://dl.cloudsmith.io/public/my-org/my-repo/cargo/index.git");
            }
            other => panic!("expected Registry, got {other:?}"),
        }
    }

    #[test]
    fn parse_source_local() {
        let source = parse_source(None, "/home/user/project/crates/foo/Cargo.toml", "/home/user/project");
        match source {
            Some(NixSource::Local { path }) => assert_eq!(path, "crates/foo"),
            other => panic!("expected Local, got {other:?}"),
        }
    }

    #[test]
    fn parse_source_local_root() {
        let source = parse_source(None, "/home/user/project/Cargo.toml", "/home/user/project");
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
        );
        match source {
            Some(NixSource::Git { url, rev, sub_dir, sha256 }) => {
                assert_eq!(url, "https://github.com/example/repo.git");
                assert_eq!(rev, "abc123def456");
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
        );
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
    fn infer_git_with_name_at_version() {
        // pkg_id format: git+URL#name@version — should extract version as rev
        // (this was a bug: previously returned "my-crate@0.5.0" as rev)
        let source = infer_source_from_pkg_id(
            "git+https://github.com/example/repo.git?rev=abc123#my-crate@0.5.0",
        );
        match source {
            Some(NixSource::Git { url, rev, .. }) => {
                assert_eq!(url, "https://github.com/example/repo.git");
                assert_eq!(rev, "0.5.0", "should strip name@ prefix from fragment");
            }
            other => panic!("expected Git, got {other:?}"),
        }
    }

    #[test]
    fn infer_git_with_bare_hash() {
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
}
