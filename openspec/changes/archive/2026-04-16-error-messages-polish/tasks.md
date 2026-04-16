## 1. Rust CLI error improvements

- [x] 1.1 `cargo.rs` — `run_unit_graph` failure: append hint "Is nightly Rust installed? `cargo build --unit-graph` requires `-Z unstable-options`"
- [x] 1.2 `cargo.rs` — `run_cargo_metadata` failure: include cargo stderr in the error context
- [x] 1.3 `cargo.rs` — `read_cargo_lock` / `hash_cargo_lock` file not found: add hint "Run `cargo generate-lockfile` or `cargo update` first"
- [x] 1.4 `prefetch.rs` — `nix-prefetch-git` not found on PATH: detect `NotFound` error kind, add "Install nix-prefetch-git or use `nix run .#update-plan` which bundles it"
- [x] 1.5 `prefetch.rs` — `nix-prefetch-git` non-zero exit: include the git URL + rev in the error, add hint "Check that the URL is reachable: `git ls-remote <url>`"
- [x] 1.6 `source.rs` — unknown source type error: ensure crate name is in the error message (not just the raw source string)

## 2. Nix consumer error improvements

- [x] 2.1 `build-from-unit-graph.nix` — stale plan error: add the regeneration command inline (already has it, verify wording is copy-pasteable)
- [x] 2.2 `build-from-unit-graph.nix` — missing override `builtins.trace`: include a ready-to-copy `extraCrateOverrides` snippet with the crate name filled in
- [x] 2.3 `fetch-source.nix` — git dep without sha256: include the exact `nix-prefetch-git --url <url> --rev <rev>` command in the trace message
- [x] 2.4 `auto.nix` / `vendor.nix` — git dep without crate-hashes.json entry: include exact `nix-prefetch-git` command with URL and rev filled in

## 3. Consistency pass

- [x] 3.1 Audit all `eprintln!` calls in Rust — ensure pattern "warning: <what> for <crate>: <detail>" is consistent
- [x] 3.2 Audit all `builtins.trace` / `builtins.throw` in Nix — ensure all prefixed with `unit2nix:` 
- [x] 3.3 Audit all `anyhow::Context` / `.with_context()` calls — ensure pattern "failed to <verb> <noun>" is consistent

## 4. Validation

- [x] 4.1 Manually trigger each error path and verify the message is actionable (use a scratch workspace)
- [x] 4.2 `cargo test` — all 41 tests pass
- [x] 4.3 `cargo clippy` — 0 warnings
- [x] 4.4 `nix flake check` — all 13 checks pass
- [x] 4.5 Update napkin with session notes
