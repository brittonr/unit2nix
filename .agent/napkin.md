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
- Done: second cleanup pass — see session 2026-03-01 #2 below

## Patterns That Work
- `cargo test` runs 17 unit tests — all pure (no cargo invocation needed)
- Tests cover: parse_pkg_id, parse_source variants, compute_git_subdir, cargo_lock_hash, git rev extraction
- Nix checks: `nix flake check` runs sample workspace build + VM tests (Linux only)
- Must `git add` new files before `nix build` — cleanSourceFilter excludes untracked files
- Nix derivation hashes for downstream consumers (VM tests, sample workspace) don't change when only src/ is refactored — proves behavior preservation
- `UnitMode` and `CrateKind` enums with `#[serde(rename_all = "kebab-case")]` + `#[serde(other)]` deserialize cleanly from cargo JSON
- `UnitTarget` convenience methods (`has_lib()`, `has_lib_like()`, etc.) are cleaner than free functions taking `&[String]`

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

## Session: 2026-03-01 #2 — Deep Cleanup
Changes made:
- **Bug fix**: `infer_source_from_pkg_id` now extracts git rev from `?rev=` query param instead of fragment's `name@version` (was using crate version as git rev)
- **Type safety**: `mode: String` → `UnitMode` enum (Build, RunCustomBuild, Other) with serde rename
- **Type safety**: `kind: Vec<String>` → `Vec<CrateKind>` enum (Lib, Rlib, Cdylib, ..., Other) with serde rename
- **Type safety**: Eliminated 4 stringly-typed `is_*` free functions, replaced with `UnitTarget` methods (`has_lib()`, `has_proc_macro()`, `has_lib_like()`, `has_bin()`, `has_custom_build()`)
- **Type safety**: `cli.manifest_path` and `cli.output` changed from `String` to `PathBuf`
- **Type safety**: `cargo.rs` functions now take `&Path` instead of `&str` for manifest paths
- **Decomposition**: Extracted `collect_features()` from merge() — uses BTreeSet for O(n log n) dedup instead of O(n²) Vec::contains
- **Decomposition**: Extracted `collect_dependencies()` from merge()
- **Decomposition**: Extracted `collect_build_dependencies()` from merge()
- **Decomposition**: Extracted `validate_references()` from merge() — chains dep + build_dep iterators (was duplicated loop)
- **DRY**: `is_lib_kind() || is_proc_macro()` 3-site duplication replaced by single `has_lib_like()` method
- **Error handling**: Added `with_context()` on output file write in main.rs
- **Error handling**: Added warning log when skipping packages with no buildable target
- **Style**: `parse_pkg_id` narrowed from `pub` to `pub(crate)`
- **Style**: merge() signature line-wrapped for readability
- **Style**: `to_string_lossy().to_string()` → `to_string_lossy().into_owned()` (avoids unconditional alloc)
- **Style**: Path manipulation in merge.rs uses `Path::parent()` and `Path::strip_prefix()` instead of string ops
- **Style**: Clippy fix: consecutive `str::replace` for `\n`/`\r` collapsed into single call
- **Style**: `BUILD_PLAN_VERSION` constant replaces magic number `1`
- **Docs**: Doc comments on all public types (NixBuildPlan, NixCrate, NixSource, NixDep, NixBinTarget, UnitGraph, Unit, UnitMode, CrateKind, UnitTarget, UnitDep, CargoMetadata, MetadataPackage, CargoLock, LockPackage)
- **Docs**: Doc comments on `parse_git_url` clarifying fragment semantics differ between source strings and pkg_ids
- **Tests**: Added 3 new tests (infer_git_with_rev_query_param, infer_git_with_bare_hash, infer_git_no_rev_with_name_at_version)
- **Verification**: `cargo test` 17/17 pass, `cargo clippy` 0 warnings, `nix build` + `nix flake check` all pass

## Session: 2026-03-01 #3 — Error handling, bat validation, archive
Changes made:
- **Error handling**: `parse_source` now returns `Result<Option<NixSource>>` — unknown/malformed sources produce `Err` instead of silent `None`
- **Error handling**: merge.rs catches Err from parse_source, logs warning, falls back to pkg_id inference
- **Error handling**: fetch-source.nix unknown source type fallback changed from silent `src` to `builtins.throw` with clear error
- **Tests**: Added `parse_source_unknown_type_errors` and `parse_source_malformed_git_errors` tests
- **Validation**: bat (168 crates) builds successfully with -sys crate overrides (libgit2-sys, libz-sys)
- **Archive**: Moved completed OpenSpec changes to archive/

## Domain Notes
- Multi-module Rust CLI (~8 files in src/) that merges cargo unit-graph + metadata + Cargo.lock into JSON
- Nix consumer in lib/build-from-unit-graph.nix + lib/fetch-source.nix
- benches/ has comparison benchmarks vs crate2nix, crane, buildRustPackage
- tests/vm/ has NixOS VM integration tests
- tests/ripgrep/ validates against real-world 34-crate workspace (pure Rust)
- tests/bat/ validates against 168-crate workspace with -sys crates (libgit2-sys, libz-sys)
