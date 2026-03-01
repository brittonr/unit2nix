## Context

unit2nix already produces a JSON build plan from `cargo build --unit-graph` + `cargo metadata`. The JSON contains 453 crates for aspen's workspace with resolved features, dependency edges (normal vs build), source types, SHA256 hashes, proc-macro flags, and binary targets. What's needed is a Nix expression that consumes this JSON and produces `buildRustCrate` derivations.

crate2nix's `build-from-json.nix` (274 lines) is a reference implementation but carries baggage: it reimplements `cfg()` evaluation in Nix (80 lines of string parsing) because crate2nix's JSON preserves platform conditions as strings. Our JSON has no platform conditions — Cargo's unit graph already filtered them out.

## Goals / Non-Goals

**Goals:**
- Build any Rust workspace from unit2nix JSON using `buildRustCrate`
- Handle all source types: crates.io (fetchurl), git (builtins.fetchGit), local path
- Wire proc-macros for host platform build (buildRustCrate's `procMacro` flag)
- Support crate renames via `crateRenames`
- Expose workspace members as `workspaceMembers.<name>.build`
- Work as a flake library (`lib.buildFromUnitGraph`) for easy adoption
- End-to-end test with a sample workspace

**Non-Goals:**
- Cross-compilation (Aspen only targets x86_64-linux)
- Dev-dependency handling (unit graph from `cargo build` excludes dev-deps)
- Crate overrides system (can be added later; start simple)
- Feature selection at Nix eval time (features are pre-resolved by Cargo)

## Decisions

### 1. No `cfg()` evaluator in Nix

The unit graph is already platform-filtered. Dependencies in the JSON are exactly what the target platform needs. This eliminates 80+ lines of fragile Nix string parsing that crate2nix requires.

### 2. Package ID as the crate identity key

The JSON uses Cargo's full package ID (e.g., `registry+...#serde@1.0.228`) as the key in the `crates` map. Dependencies reference these keys. The Nix consumer builds a `self`-referencing attrset where each crate looks up its deps by package ID.

### 3. Proc-macro host build via `buildRustCrate`

`buildRustCrate` natively handles proc-macros when `procMacro = true` — it builds them for the host platform. No special Nix logic needed beyond setting the flag.

### 4. SHA256 from Cargo.lock checksums

unit2nix extracts SHA256 checksums from `Cargo.lock` for crates.io dependencies. These are the same hashes `fetchurl` needs. No `nix-prefetch-url` step required.

### 5. Source fetching split into a separate file

`lib/fetch-source.nix` handles the three source types. This keeps the main consumer focused on wiring derivations.

### 6. Sample workspace for testing

A `sample_workspace/` with real Cargo.toml/Cargo.lock files and a pre-generated `build-plan.json` lets us run `nix build` in CI without needing the Rust toolchain or `cargo build --unit-graph` at eval time.

## Risks / Trade-offs

### `buildRustCrate` maintenance

`buildRustCrate` in nixpkgs has historically been less maintained than crane/naersk. Proc-macro edge cases, build script environment variables, and newer Cargo features (artifact deps, edition 2024) may need workarounds. Mitigation: the sample workspace covers the common cases; Aspen-specific issues can be handled via crate overrides.

### Nightly-only `--unit-graph`

The `--unit-graph` flag requires `-Z unstable-options` (nightly Cargo). It has tracking issue #8002 with no stabilization timeline. Aspen already uses nightly, so this isn't a blocker, but it limits adoption for stable-toolchain projects.

### SHA256 hash format

`Cargo.lock` v3 uses SHA256 hex checksums. `buildRustCrate` and `fetchurl` expect the same format. If the Cargo.lock format changes, unit2nix's parser needs updating.

### Build script environment

`buildRustCrate` sets standard Cargo build script environment variables but may miss newer ones. Build scripts that depend on `CARGO_ENCODED_RUSTFLAGS` or artifact dependency variables may fail. Mitigation: Aspen's build scripts are simple (version detection, codegen) and work with `buildRustCrate`.
