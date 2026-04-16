## Why

Aspen's 72-crate workspace currently uses crane for Nix builds, which does a single `cargo build` invocation per derivation. Changing one crate rebuilds everything. Per-crate Nix caching via `buildRustCrate` would let unchanged crates be served from the Nix store, dramatically reducing CI rebuild times.

The unit2nix tool already generates a build plan JSON by merging Cargo's unit graph (exact resolved features, deps, platform filtering) with cargo metadata (SHA256 hashes, source info). What's missing is the Nix consumer that turns this JSON into `buildRustCrate` derivations.

## What Changes

- Add `lib/build-from-unit-graph.nix` — the Nix consumer that reads unit2nix JSON output and produces per-crate `buildRustCrate` derivations
- Add `lib/fetch-source.nix` — source fetching logic for crates.io (fetchurl), git (fetchGit), and local path dependencies
- Add an integration test that builds a small sample workspace end-to-end through unit2nix → Nix
- Add a `sample_workspace/` with 3-4 crates (lib, bin, proc-macro, build script) to validate the full pipeline
- Add a flake output `lib.buildFromUnitGraph` so other flakes can consume this

## Capabilities

### New Capabilities
- `nix-consumer`: Nix expression that reads unit2nix JSON and builds a workspace using `buildRustCrate` per-crate derivations. Handles source fetching, dependency wiring, proc-macro host builds, build script execution, crate renames, and binary target collection.
- `sample-workspace`: Minimal multi-crate Rust workspace for end-to-end testing of the unit2nix → Nix pipeline. Covers lib, bin, proc-macro, build script, and feature flag scenarios.

### Modified Capabilities

## Impact

- New Nix files under `lib/`
- New `sample_workspace/` directory with test crates
- Flake outputs extended with `lib.buildFromUnitGraph`
- No changes to existing Rust code in `src/main.rs`
- Downstream consumers (aspen) can adopt by importing the flake and calling `buildFromUnitGraph { src = ./.; resolvedJson = ./build-plan.json; }`
