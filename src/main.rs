mod cargo;
mod cli;
mod merge;
mod metadata;
mod output;
mod prefetch;
mod source;
mod unit_graph;

use anyhow::Result;
use clap::Parser;

use cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();

    eprintln!("Running cargo build --unit-graph...");
    let unit_graph = cargo::run_unit_graph(&cli)?;
    eprintln!("  {} units, {} roots", unit_graph.units.len(), unit_graph.roots.len());

    eprintln!("Running cargo metadata...");
    let metadata = cargo::run_cargo_metadata(&cli)?;
    eprintln!("  {} packages", metadata.packages.len());

    eprintln!("Reading Cargo.lock...");
    let lock = cargo::read_cargo_lock(&cli.manifest_path)?;
    eprintln!(
        "  {} packages with checksums",
        lock.package
            .as_ref()
            .map(|p| p.iter().filter(|p| p.checksum.is_some()).count())
            .unwrap_or(0)
    );

    eprintln!("Hashing Cargo.lock...");
    let cargo_lock_hash = cargo::hash_cargo_lock(&cli.manifest_path)?;
    eprintln!("  sha256: {cargo_lock_hash}");

    eprintln!("Merging...");
    let mut plan = merge::merge(&unit_graph, &metadata, &lock, cli.target.as_deref(), cargo_lock_hash)?;
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
            std::fs::write(path, &json)?;
            eprintln!("Wrote {path}");
        }
        None => println!("{json}"),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::cargo::hash_cargo_lock;
    use crate::merge::parse_pkg_id;
    use crate::output::NixSource;
    use crate::source::{compute_git_subdir, parse_source};

    #[test]
    fn parse_registry_pkg_id() {
        let (name, version) =
            parse_pkg_id("registry+https://github.com/rust-lang/crates.io-index#serde@1.0.200");
        assert_eq!(name, "serde");
        assert_eq!(version, "1.0.200");
    }

    #[test]
    fn parse_path_pkg_id() {
        let (name, version) =
            parse_pkg_id("path+file:///home/user/project/crates/aspen-core#0.1.0");
        assert_eq!(name, "aspen-core");
        assert_eq!(version, "0.1.0");
    }

    #[test]
    fn parse_git_pkg_id() {
        let (name, version) =
            parse_pkg_id("git+https://github.com/example/repo.git?rev=abc123#my-crate@0.5.0");
        assert_eq!(name, "my-crate");
        assert_eq!(version, "0.5.0");
    }

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
    fn cargo_lock_hash_is_sha256_hex() {
        // Hash our own Cargo.lock as a smoke test
        let hash = hash_cargo_lock("./Cargo.toml").expect("should hash Cargo.lock");
        assert_eq!(hash.len(), 64, "SHA256 hex should be 64 chars, got: {hash}");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hash should be hex, got: {hash}"
        );
    }

    #[test]
    fn cargo_lock_hash_is_deterministic() {
        let h1 = hash_cargo_lock("./Cargo.toml").unwrap();
        let h2 = hash_cargo_lock("./Cargo.toml").unwrap();
        assert_eq!(h1, h2, "same file should produce same hash");
    }
}
