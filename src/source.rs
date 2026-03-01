use crate::output::NixSource;

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
            // Format: git+URL?rev=HASH#HASH or git+URL#HASH
            let without_prefix = s.strip_prefix("git+").unwrap_or(s);
            let (url_part, rev) = if let Some((url, hash)) = without_prefix.rsplit_once('#') {
                (url.to_string(), hash.to_string())
            } else {
                (without_prefix.to_string(), String::new())
            };
            // Strip query params from URL
            let url = url_part.split('?').next().unwrap_or(&url_part).to_string();

            // Compute subdirectory from manifest_path.
            // Cargo's git checkout lives at ~/.cargo/git/checkouts/<repo>/<hash>/
            // The manifest_path is e.g. ~/.cargo/git/checkouts/repo/abc123/crates/foo/Cargo.toml
            // We want sub_dir = "crates/foo"
            let sub_dir = compute_git_subdir(manifest_path);

            Some(NixSource::Git { url, rev, sub_dir, sha256: None })
        }
        _ => None,
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
/// - `git+https://...#hash` → git
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
        let without_prefix = pkg_id.strip_prefix("git+")?;
        let (url_part, rev) = without_prefix.rsplit_once('#')?;
        let url = url_part.split('?').next()?.to_string();
        Some(NixSource::Git {
            url,
            rev: rev.to_string(),
            sub_dir: None,
            sha256: None,
        })
    } else {
        // path+ or unknown — local
        None
    }
}
