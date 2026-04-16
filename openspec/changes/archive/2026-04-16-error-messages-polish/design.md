# Error Messages Polish — Design

## Approach

Audit every error/warning path across Rust CLI and Nix consumers. For each, apply a consistent format:

```
unit2nix: <what failed>
  <why it failed>
  <how to fix it>
```

## Error Path Inventory

### Rust CLI (`src/`)

| Location | Current | Improvement |
|----------|---------|-------------|
| `cargo.rs` — `run_unit_graph` failure | Shows truncated stdout + stderr | Add: "Is nightly Rust installed? `cargo build --unit-graph` requires `-Z unstable-options`" |
| `cargo.rs` — `run_cargo_metadata` failure | Raw anyhow error | Add cargo stderr + "check that Cargo.toml is valid" |
| `cargo.rs` — `read_cargo_lock` missing file | "failed to read Cargo.lock" | Add: "Run `cargo generate-lockfile` first" |
| `cargo.rs` — `hash_cargo_lock` failure | Raw I/O error | Same as above |
| `prefetch.rs` — `nix-prefetch-git` not found | Process spawn error | Add: "Install nix-prefetch-git or use `nix run .#update-plan` which bundles it" |
| `prefetch.rs` — `nix-prefetch-git` fails | Raw stderr | Add: "Check that the git URL is reachable: `git ls-remote <url>`" |
| `source.rs` — unknown source type | `Err("unknown source type: ...")` | Add crate name + the raw source string for debugging |
| `merge.rs` — dangling dep reference | Index out of bounds with context | Already good — verify message is clear |
| `run.rs` — `--check-overrides` without `-o` | "requires -o" | Already good |
| `merge.rs` — `parse_pkg_id` malformed | "malformed package ID" | Already good — includes the pkg_id |

### Nix consumers (`lib/`)

| Location | Current | Improvement |
|----------|---------|-------------|
| `build-from-unit-graph.nix` — stale plan | `builtins.throw` with hashes | Add: show diff of expected vs got hash, include `nix run .#update-plan` command |
| `build-from-unit-graph.nix` — target mismatch | `builtins.trace` warning | Already good |
| `build-from-unit-graph.nix` — missing override | `builtins.trace` warning | Upgrade: include the exact `extraCrateOverrides` snippet to copy-paste |
| `fetch-source.nix` — alternative registry | `builtins.throw` with template | Already good |
| `fetch-source.nix` — git dep no sha256 | `builtins.trace` + `builtins.fetchGit` | Add: exact `nix-prefetch-git --url <url> --rev <rev>` command |
| `fetch-source.nix` — unknown source type | `builtins.throw` | Already good |
| `auto.nix` — git dep without hash | `builtins.throw` with instructions | Add: exact `nix-prefetch-git` command with URL + rev filled in |
| `vendor.nix` — missing crate hash | Probably silent or cryptic | Audit and add clear error |

### Consistency pass

- All Rust errors should use `anyhow::Context` with the pattern: "failed to <verb> <noun>: <detail>"
- All Nix errors should prefix with `unit2nix:` for grep-ability
- All warnings should use `eprintln!("warning: ...")` in Rust and `builtins.trace "unit2nix: WARNING — ..."` in Nix

## Non-goals

- Not changing error *handling* logic (that's already solid from session #3 and #6)
- Not adding new error types or Result variants
- Not changing exit codes
