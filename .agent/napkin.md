# Napkin

## Corrections
| Date | Source | What Went Wrong | What To Do Instead |
|------|--------|----------------|-------------------|
| 2026-03-01 | self | Forgot `mod unit_graph;` in main.rs module declarations | Double-check all mod declarations match created files |
| 2026-03-01 | self | Put `pub` on enum variant fields in output.rs (Rust doesn't allow this) | Enum variant fields are inherently public when enum is pub |
| 2026-03-01 | self | Removed NixSource import from merge.rs but it's used by parse_source return type | Check if types flow through function return types before removing imports |
| 2026-03-01 | self | merge.rs needed type annotation `Option<&(usize, &Unit)>` for lib_unit | Complex iterator chains with .or_else need explicit type annotations when types move across modules |

## User Preferences
- Priority: split monolithic main.rs into modules, extract merge() helpers, clean dead_code, DRY cargo commands
- Done: code cleanup pass — see session 2026-03-01 below

## Patterns That Work
- `cargo test` runs 14 unit tests from src/main.rs — all pure (no cargo invocation needed)
- Tests cover: parse_pkg_id, parse_source variants, compute_git_subdir, cargo_lock_hash
- Nix checks: `nix flake check` runs sample workspace build + VM tests (Linux only)
- Must `git add` new files before `nix build` — cleanSourceFilter excludes untracked files
- Nix derivation hashes for downstream consumers (VM tests, sample workspace) don't change when only src/ is refactored — proves behavior preservation

## Patterns That Don't Work
- (none yet)

## Session: 2026-03-01 Cleanup
Changes made:
- **Bug fix**: `infer_source_from_pkg_id` was returning `name@version` as rev for git deps; now uses shared `parse_git_url()` that strips the name@ prefix
- **Bug fix**: `.unwrap()` in prefetch.rs → `.expect()` with message
- **DRY**: Extracted `cargo_lock_path()` helper (was duplicated in `read_cargo_lock` and `hash_cargo_lock`)
- **DRY**: Extracted `is_proc_macro()`, `is_bin()`, `is_custom_build()` predicates (5 `.contains(&"proc-macro".to_string())` calls eliminated)
- **DRY**: Extracted `parse_git_url()` shared by `parse_source` and `infer_source_from_pkg_id`
- **DRY**: `optionalField` helper in build-from-unit-graph.nix replaces 9 repetitive `optionalAttrs` blocks
- **Error handling**: `run_cargo` now includes stdout excerpt on failure
- **Error handling**: Unknown source types now `eprintln!` a warning instead of silently returning None
- **Error handling**: Fixed misleading registry error message in fetch-source.nix
- **Structure**: Moved all 14 tests from main.rs to their respective modules (source.rs, merge.rs, cargo.rs)
- **Structure**: Added 2 new tests for the git rev parsing fix
- **Dead code**: Removed unused MetadataPackage fields (name, version, targets), unused MetadataTarget struct, unused LockPackage.source, unused UnitDep.public
- **Visibility**: `is_lib_kind` and `PrefetchGitResult` narrowed from `pub` to private
- **Style**: Cleaned up arg building in `run_unit_graph` using `as_deref()` instead of temp String clones
- **Nix**: Renamed `_crateInfo` → `_` in mapAttrs (unused parameter)

## Domain Notes
- Single-file Rust CLI (src/main.rs, 590 lines) that merges cargo unit-graph + metadata + Cargo.lock into JSON
- Nix consumer in lib/build-from-unit-graph.nix + lib/fetch-source.nix
- benches/ has comparison benchmarks vs crate2nix, crane, buildRustPackage
- tests/vm/ has NixOS VM integration tests
- tests/ripgrep/ validates against real-world 34-crate workspace
