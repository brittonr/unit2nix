## 1. Built-in Override Registry

- [x] 1.1 Create `lib/crate-overrides.nix` with `{ pkgs }: attrset` exporting overrides for: `libgit2-sys`, `libz-sys`, `libsqlite3-sys`, `ring`, `onig_sys`, `tikv-jemalloc-sys`, `prost-build`, `openssl-sys`, `libssh2-sys`, `curl-sys`, `libdbus-sys`, `rdkafka-sys`, `librocksdb-sys`
- [x] 1.2 Add `knownNoOverride` set in `lib/crate-overrides.nix` for Rust-internal links crates: `rayon-core`, `prettyplease02`, `ring_core_*` pattern
- [x] 1.3 Expose `lib.crateOverrides` and `lib.knownNoOverride` in `flake.nix` outputs

## 2. Override Merging in build-from-unit-graph.nix

- [x] 2.1 Add `extraCrateOverrides ? {}` parameter to `buildFromUnitGraph`
- [x] 2.2 Implement three-layer merge: `pkgs.defaultCrateOverrides // unit2nixOverrides // extraCrateOverrides` as the new default when `defaultCrateOverrides` is not explicitly passed
- [x] 2.3 Preserve backward compatibility: when `defaultCrateOverrides` is explicitly passed, use it directly (skip built-in layer), then merge `extraCrateOverrides` on top
- [x] 2.4 Wire the merged overrides into `buildRustCrate.override`

## 3. Eval-time Diagnostics

- [x] 3.1 In `buildCrate`, check if crate has `links` field and `crateName` is not in merged overrides or `knownNoOverride` — emit `builtins.trace` warning with crate name, links value, and docs pointer
- [x] 3.2 Test that covered crates (libz-sys, ring) produce no warning
- [x] 3.3 Test that `rayon-core` and `prettyplease02` produce no warning (knownNoOverride)

## 4. CLI --check-overrides Flag

- [x] 4.1 Add `--check-overrides` flag to `src/cli.rs` (reads build plan JSON path)
- [x] 4.2 Add compiled-in known-crate registry in `src/overrides.rs` — `HashMap<&str, &str>` mapping crate names to human-readable notes
- [x] 4.3 Implement check logic: scan `crates` for `links` fields, cross-reference against known registry, print coverage report
- [x] 4.4 Wire into `src/run.rs` — when `--check-overrides` is set, run check and exit (skip normal generation)

## 5. Simplify Test Builds

- [x] 5.1 Update `tests/bat/build.nix` — remove `libgit2-sys` and `libz-sys` overrides (now built-in), verify build passes
- [x] 5.2 Update `tests/nushell/build.nix` — remove `libsqlite3-sys` and `ring` overrides, verify build passes
- [x] 5.3 Update `tests/fd/build.nix` — check if `tikv-jemalloc-sys` override is now covered by built-in
- [x] 5.4 Run `nix flake check` — all 9 checks must pass

## 6. Documentation

- [x] 6.1 Rewrite `docs/sys-crate-overrides.md` — lead with "works out of the box", document override hierarchy, `extraCrateOverrides`, `--check-overrides`
- [x] 6.2 Update README.md quickstart and override examples to use `extraCrateOverrides`
- [x] 6.3 Update napkin with session notes
