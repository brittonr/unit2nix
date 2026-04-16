## Why

Users must manually figure out which crates need native overrides, look up the correct nixpkgs package names, and write boilerplate Nix override functions. For a project like nushell (519 crates), the user has to read build failures, cross-reference crate names with nixpkgs, and hand-write override blocks — a painful trial-and-error loop. We can ship a curated override registry and surface actionable diagnostics so most projects "just work" or tell the user exactly what to add.

## What Changes

- **Built-in override registry** (`lib/crate-overrides.nix`): A curated attrset of common -sys crate overrides (openssl-sys, libgit2-sys, libz-sys, libsqlite3-sys, ring, prost-build, etc.) that unit2nix ships alongside nixpkgs' `defaultCrateOverrides`. Users inherit both automatically — no boilerplate for well-known crates.
- **`links`-based sys crate detection**: At eval time, build-from-unit-graph.nix inspects each crate's `links` field to identify -sys crates. Any -sys crate with `links` that has no matching override (from nixpkgs defaults, unit2nix built-ins, or user overrides) emits a `builtins.trace` warning with the exact crate name and a suggested override skeleton.
- **`--check-overrides` CLI flag**: A dry-run mode in the Rust CLI that scans the build plan JSON and reports which crates have `links` fields, which have known overrides, and which are missing — before the user even tries to build.
- **Simplified override merging**: `buildFromUnitGraph` gains an `extraCrateOverrides` parameter that merges on top of the built-in + nixpkgs defaults, so users only specify their project-specific additions.

## Capabilities

### New Capabilities
- `builtin-overrides`: Curated registry of common -sys crate overrides shipped with unit2nix
- `override-diagnostics`: Eval-time warnings and CLI check mode for missing sys crate overrides

### Modified Capabilities

## Impact

- `lib/build-from-unit-graph.nix`: New default merging logic, `extraCrateOverrides` param, `links`-based trace warnings
- `lib/crate-overrides.nix`: New file with the curated override registry
- `src/cli.rs`: New `--check-overrides` flag
- `src/run.rs` or new `src/check.rs`: Logic to scan build plan for unmatched `links` fields
- `docs/sys-crate-overrides.md`: Rewritten to lead with "it just works" and document the override hierarchy
- `flake.nix`: Expose `lib.crateOverrides` for downstream consumers
- Test builds (bat, nushell, fd): Simplified — most overrides become unnecessary since built-ins cover them
- **No breaking changes**: Existing `defaultCrateOverrides` parameter continues to work
