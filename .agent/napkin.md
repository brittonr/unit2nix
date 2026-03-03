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
- `cargo test` runs 20 unit tests — all pure (no cargo invocation needed)
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

## Session: 2026-03-01 #4 — cargo subcommand + IFD auto-build
Changes made:
- **cargo-unit2nix**: second `[[bin]]` entry with separate `cargo_main.rs` that strips cargo's inserted subcommand arg
- **nix run .#update-plan**: flake app wrapping unit2nix for zero-install regen
- **Wrapper fix**: `--prefix PATH` for cargo/rustc (user's `~/.local/bin/cargo` was a shell wrapper that polluted stdout via shellHook)
- **IFD auto-build**: `buildFromUnitGraphAuto` — vendors crate sources from Cargo.lock checksums, runs unit2nix in sandbox, IFDs result
- **lib/vendor.nix**: parses Cargo.lock via `lib.importTOML`, fetches crates.io via `fetchurl`, git via `fetchgit`/`builtins.fetchGit`
- **lib/auto.nix**: orchestrates vendor → unit2nix → buildFromUnitGraph with `skipStalenessCheck = true`
- **Validation**: 7 flake checks pass (new: `sample-auto`), auto mode produces identical binaries to manual mode

Lessons:
- Two `[[bin]]` entries sharing same `main.rs` produces cargo warning — use separate entry point file
- `cargo unit2nix` invocation passes extra arg (`cargo-unit2nix unit2nix -o foo`) — must strip it
- IFD ≠ impure. IFD is orthogonal to pure eval. Only Hydra blocks IFD (`allow-import-from-derivation = false`)
- User's cargo wrapper (`~/.local/bin/cargo`) runs `nix develop --command cargo` which echoes shellHook to stdout, corrupting piped JSON — fix by `--prefix PATH` with clean Nix cargo

## Session: 2026-03-01 #3 — Error handling, bat validation, archive
Changes made:
- **Error handling**: `parse_source` now returns `Result<Option<NixSource>>` — unknown/malformed sources produce `Err` instead of silent `None`
- **Error handling**: merge.rs catches Err from parse_source, logs warning, falls back to pkg_id inference
- **Error handling**: fetch-source.nix unknown source type fallback changed from silent `src` to `builtins.throw` with clear error
- **Tests**: Added `parse_source_unknown_type_errors` and `parse_source_malformed_git_errors` tests
- **Validation**: bat (168 crates) builds successfully with -sys crate overrides (libgit2-sys, libz-sys)
- **Archive**: Moved completed OpenSpec changes to archive/

## Session: 2026-03-02 #5 — Git dep handling in auto mode, Aspen validation
Changes made:
- **workspaceDir param**: `auto.nix` + flake.nix support `workspaceDir` for projects with external path deps (`../sibling`)
- **Git dep strategy**: vendor.nix no longer puts git deps in linkFarm (cargo's directory vendor format can't handle workspace inheritance like `rust-version.workspace = true`)
- **Fake git wrapper**: auto.nix provides a shell script that intercepts `git clone`/`fetch` and serves from pre-fetched nix store paths; cargo uses it via `net.git-fetch-with-cli = true`
- **fetchgit leaveDotGit**: git deps with sha256 in crate-hashes.json use `fetchgit { leaveDotGit = true; }` to preserve `.git` for the wrapper
- **Clear error for missing hashes**: git deps without crate-hashes.json entry get `builtins.throw` with exact instructions
- **Aspen validated**: 1,359 crates, 5 git deps (snix, iroh-experiments, iroh-proxy-utils, mad-turmoil, wu-manber), external path dep (aspen-wasm-plugin) — full auto build succeeded

Lessons:
- `builtins.fetchGit` strips `.git` — can't use the result as a git remote. Must use `fetchgit { leaveDotGit = true; }` which requires sha256
- Cargo's git cache dir names use SipHash of canonical URL — impossible to replicate in pure Nix. Intercepting git CLI is simpler
- `cp -r ${src} source` loses parent context — `workspaceDir` param lets auto.nix copy the full parent tree and find Cargo.toml at the right relative path
- `rawSrc`/`src` defined inside an attrset literal aren't accessible to each other — must use `let...in` for self-referencing bindings

## Session: 2026-03-02 #6 — Code review cleanup (15 issues)
Changes made:
- **Bug fix (critical)**: `&stdout[..500]` byte-slicing in cargo.rs error path → char-boundary-safe truncation via `char_indices().nth(500)`
- **Bug fix (high)**: `wm.starts_with(pkg_id)` for workspace member detection → exact equality `wm == pkg_id` (prevented false prefix match on e.g. `bar` matching `bar-baz`)
- **Bug fix (high)**: String-based path manipulation in source.rs `parse_source` → proper `Path::strip_prefix()` (was missed in session #2 which fixed merge.rs)
- **DRY (high)**: Extracted `src/run.rs` with shared `run(cli)` function — `main.rs` and `cargo_main.rs` both call it, eliminating 43 lines of duplicated orchestration logic
- **Error handling**: `parse_pkg_id` now returns `Result` instead of `("unknown", "0.0.0")` sentinel — malformed pkg_ids produce clear errors instead of silent corruption
- **Error handling**: Warning log when both `parse_source` and `infer_source_from_pkg_id` return `None` for non-local deps
- **Error handling**: Bounds-checked index access on `unit_graph.units[dep.index]` in `collect_dependencies` — panics now include dep index, array length, and pkg_id context
- **Error handling**: Bounds-checked roots index access in merge()
- **Error handling**: `fetch-source.nix` git fallback now emits `builtins.trace` warning when using `builtins.fetchGit` (no sha256)
- **Consistency**: `collect_build_dependencies` now deduplicates with `HashSet` (matching `collect_dependencies`)
- **Dead code**: Removed unreachable `split('#').next().unwrap_or("")` in `parse_pkg_id` — reused existing `prefix` binding from `rsplit_once('#')`
- **Nix**: Template `flake.nix` now uses `flake-utils.lib.eachDefaultSystem` instead of hardcoded `x86_64-linux`
- **Nix**: `rootCrate` in `build-from-unit-graph.nix` now has a comment explaining it only exposes the first root
- **Tests**: Added `parse_pkg_id_malformed_no_hash` test
- **Verification**: `cargo test` 20/20 pass, `cargo clippy` 0 warnings, `nix build` + `nix flake check` (7 checks) all pass

Lessons:
- Must `git add` new files (run.rs) before `nix build` — cleanSourceFilter excludes untracked
- `str::char_indices().nth(n)` is the safe way to truncate a string by character count

## Session: 2026-03-03 — Validation targets, cross-compilation, README fixes
Changes made:
- **Validation**: Added fd (59 crates, 1 workspace member, jemalloc-sys) — builds and runs
- **Validation**: Added nushell (519 crates, 29 workspace members, sqlite+ring+rmcp) — builds and runs
- **Cross-compilation**: Added target mismatch warning in build-from-unit-graph.nix — traces when build plan target ≠ pkgs.hostPlatform.config
- **README**: Fixed test count 19 → 20
- **README**: Updated tested projects table with fd and nushell
- **README**: Enhanced cross-compilation docs with target mismatch warning, multiple code examples, and design rationale
- **README**: Updated check count 7 → 9
- **Flake**: Added validate-fd and validate-nushell checks
- **Verification**: cargo test 20/20, cargo clippy 0 warnings, all 5 validation builds pass (sample, ripgrep, fd, bat, nushell)

Lessons:
- `RING_PREGENERATE_ASM=1` is WRONG — it tells ring to regenerate asm, which fails when pregenerated/ dir exists. Remove it; the source tarball ships pregenerated asm
- `rmcp` crate uses `env!("CARGO_CRATE_NAME")` at compile time — buildRustCrate doesn't set this; must provide via override
- nushell's build.rs reads `CARGO_CFG_FEATURE` — buildRustCrate doesn't set CARGO_CFG_* vars; must provide via override
- fd's crate name is `fd-find` not `fd` (check workspaceMembers keys in the JSON)

## Session: 2026-03-03 #2 — Env var shim, template test, benchmarks, flake check
Changes made:
- **Env var shim**: build-from-unit-graph.nix now auto-sets `CARGO_CRATE_NAME` and `CARGO_CFG_FEATURE` for every crate build — eliminates need for per-crate overrides when crates use `env!()` or `std::env::var()` for these vars
- **Removed overrides**: nushell test no longer needs rmcp/nu overrides (only sqlite + ring remain)
- **Docs**: Updated sys-crate-overrides.md with env var shim documentation, ring `RING_PREGENERATE_ASM` warning, added nushell/fd as working examples
- **Template test**: Verified `nix flake init -t` → `unit2nix -o build-plan.json` → `nix build` works end-to-end with a real serde project
- **Benchmarks**: Refreshed generate/eval numbers (consistent: unit2nix eval 308ms ≈ crate2nix 311ms, crane 864ms, buildRustPackage 482ms)
- **Full flake check**: All 9 checks pass including 3 VM tests (sample-bin, per-crate-caching, rebuild-isolation)
- **Gitignore**: Added result-1 to .gitignore

Lessons:
- `buildRustCrate` sets `CARGO_FEATURE_*` (per-feature) and `CARGO_PKG_*` and `CARGO_CFG_TARGET_*` but NOT `CARGO_CRATE_NAME` or `CARGO_CFG_FEATURE`
- These two env vars can be computed from the build plan JSON — `CARGO_CRATE_NAME` is crateName with `-` → `_`, `CARGO_CFG_FEATURE` is comma-separated features list
- Flake template correctly produces `.gitignore` and `flake.nix` but overwrites existing `.gitignore` (nix warns about it)

## Session: 2026-03-03 #3 — Sys crate override ergonomics
Changes made:
- **Built-in override registry**: New `lib/crate-overrides.nix` with overrides for 8 crates not in nixpkgs: ring, tikv-jemalloc-sys, jemalloc-sys, onig_sys, librocksdb-sys, zstd-sys, bzip2-sys, lzma-sys
- **knownNoOverride set**: rayon-core, prettyplease, compiler_builtins, etc. plus `ring_core_*` prefix pattern — suppresses false-positive warnings
- **Three-layer merge**: `pkgs.defaultCrateOverrides // unit2nix built-ins // extraCrateOverrides` — new default when `defaultCrateOverrides` is not explicitly passed
- **extraCrateOverrides param**: New additive parameter on `buildFromUnitGraph` and `buildFromUnitGraphAuto` — users only write project-specific overrides
- **Backward compat**: `defaultCrateOverrides` param still works — when passed, replaces layers 1+2 entirely
- **Eval-time diagnostics**: `builtins.trace` warning for crates with `links` field and no matching override or knownNoOverride entry
- **--check-overrides CLI**: Reads build plan JSON and prints coverage report — covered/no-override-needed/missing
- **Deserialize support**: Added `Deserialize` to all output types for `--check-overrides` to read back build plan JSON
- **Simplified test builds**: bat (removed 2 overrides), nushell (removed 2 overrides), fd (removed 1 override) — all now use defaults
- **Documentation**: Rewrote sys-crate-overrides.md to lead with "works out of the box", updated README API docs and tested projects table
- **Verification**: cargo test 20/20, cargo clippy 0 warnings, nix flake check all 9 checks pass

Lessons:
- nixpkgs' `defaultCrateOverrides` already covers ~60 crates including libgit2-sys, libz-sys, libsqlite3-sys, openssl-sys — don't duplicate those
- `defaultCrateOverrides ? null` (not `? pkgs.defaultCrateOverrides`) to detect "user didn't pass it" vs "user passed nixpkgs defaults"
- Build plan JSON needs `#[serde(default)]` on optional fields for backward-compatible deserialization of older plans
- ring's `links` value is `ring_core_0_17_14_` (version-stamped), not `ring` — need prefix matching for knownNoOverride

## Session: 2026-03-03 #4 — DRY extraction + clippy pedantic cleanup
Changes made:
- **DRY (major)**: Extracted `build_nix_crate()` — shared NixCrate construction helper eliminates ~60 lines of duplicated logic between `merge()` and `compute_dev_dependencies()`
- **DRY**: Extracted `resolve_source()` — shared source resolution with warning logging
- **DRY**: Extracted `sanitize_metadata()` — was duplicated as inline closure in both functions
- **DRY**: Extracted `make_relative()` — was duplicated as inline closure in both functions
- **DRY**: Extracted `index_build_deps()` and `index_test_deps()` — unit graph indexing pulled out of `compute_dev_dependencies()` to keep it under 100 lines
- **Clippy pedantic**: Fixed all 33 warnings → 0 remaining:
  - 10 redundant closures → method references (`CrateKind::is_lib`, `String::as_str`, `str::to_owned`, etc.)
  - 6 doc backtick issues → backticked `NixSource`, `manifest_path`, `pkg_id`, `pkg_ids`
  - 4 inline format vars → `format!("{description}")` style
  - 3 `map().unwrap_or_else()` → `map_or_else()` / `map_or()`
  - 2 `let...else` opportunities → `let Some(..) = .. else { continue }`
  - 1 redundant else block (bail! never falls through)
  - 1 pass-by-value not consumed → `run(cli: &Cli)` (was taking ownership unnecessarily)
  - 1 implicit string clone → `.clone()` instead of `.to_string()` on `&String`
  - 1 explicit iter loop → `&container` shorthand
  - 1 excessive bools → `#[allow]` on CLI args struct (expected for clap)
  - 1 `format!("{:x}", hash)` → `format!("{hash:x}")`
- **Verification**: cargo test 20/20, cargo clippy 0 warnings, cargo clippy pedantic 0 warnings, nix flake check 11/11 pass

Lessons:
- When changing a function from `fn f(x: T)` to `fn f(x: &T)`, update all `&x` references inside the body to `x` to avoid double-referencing (`&&T`)
- Explicit lifetime annotations `<'a>` on functions that take a single reference are redundant — Rust's elision rules handle it
- `str::to_owned` is the idiomatic method reference for `|s: &str| s.to_string()`

## Session: 2026-03-03 #5 — Test coverage + Nix test execution
Changes made:
- **Test coverage**: Added 21 new unit tests for merge.rs (was 4, now 25 in merge, 41 total project)
  - `make_relative` (2 tests): prefix stripping, mismatch fallback
  - `sanitize_metadata` (2 tests): newline replacement, quote replacement
  - `collect_features` (3 tests): dedup+sort, Build-mode-only, lib-like-only
  - `collect_dependencies` (3 tests): self-ref filtering, RunCustomBuild skip, dedup
  - `collect_build_dependencies` (2 tests): from build script, empty when None
  - `validate_references` (2 tests): passes on valid, fails on dangling
  - `build_nix_crate` (3 tests): lib crate, bin-only crate, skips no-buildable-target
  - `compute_dev_dependencies` (2 tests): identifies dev-only deps, adds dev-only crates
  - `merge` (2 tests): end-to-end with workspace fixture, workspace_members mapping
- **Test fixture helpers**: `make_unit()`, `make_meta_pkg()`, `make_lock_pkg()` for easy fixture construction
- **Nix test execution**: `test.check` attribute runs `#[test]` functions inside Nix sandbox
  - Uses `.override { buildTests = true; }` on testCrates (no code duplication)
  - Dependencies built normally (same store paths); only workspace member recompiled with `--test`
  - Test binaries in `$out/tests/` executed via `runCommand`
- **sample-lib**: Now uses `pretty_assertions::assert_eq!` (validates dev-dep linking actually works)
- **Flake checks**: Added `sample-run-tests` and `sample-run-tests-bin` (13 total checks)
- **README**: Updated test count 20→41, check count 9→13, added `test.check`/`test.workspaceMembers`/`clippy.allWorkspaceMembers` to API table, added "Running tests in Nix" section
- **Verification**: cargo test 41/41, cargo clippy pedantic 0 warnings, nix flake check 13/13

Lessons:
- `buildRustCrate` supports `buildTests = true` which compiles with `--test` and installs test binaries to `$out/tests/` — does NOT run them
- `.override { buildTests = true; }` on a buildRustCrate result works because it's wrapped in `makeOverridable` — no need to duplicate the entire build function
- When `buildTests = true`, the derivation only has `out` (no `lib`), so test builds can't be used as deps — each test build must link against normal builds

## Domain Notes
- Multi-module Rust CLI (~8 files in src/) that merges cargo unit-graph + metadata + Cargo.lock into JSON
- Nix consumer in lib/build-from-unit-graph.nix + lib/fetch-source.nix
- benches/ has comparison benchmarks vs crate2nix, crane, buildRustPackage
- tests/vm/ has NixOS VM integration tests
- tests/ripgrep/ validates against real-world 34-crate workspace (pure Rust)
- tests/bat/ validates against 168-crate workspace with -sys crates (libgit2-sys, libz-sys)
