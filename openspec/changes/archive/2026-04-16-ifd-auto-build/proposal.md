## Why

unit2nix currently requires a manual regeneration step: run unit2nix, commit `build-plan.json`, then build. The `update-plan-app` change (in progress) reduces friction but doesn't eliminate the step.

crate2nix's `generatedCargoNix` / `tools.nix` shows this can be fully automated via IFD (Import From Derivation): vendor crate sources from `Cargo.lock` checksums, run the generator in a sandboxed derivation, import the result at eval time. No manual step, no user-provided hashes, and it's pure eval compatible.

IFD is enabled by default in Nix. The only environment that blocks it is Hydra (`allow-import-from-derivation = false`). For the majority of users not on Hydra, this is zero-friction per-crate builds.

## What Changes

- Add `buildFromUnitGraphAuto` function to `lib/` that vendors deps and runs unit2nix via IFD
- Add a vendoring module that parses `Cargo.lock` at eval time and fetches crate sources as FODs
- Expose `buildFromUnitGraphAuto` alongside existing `buildFromUnitGraph` in the flake lib output
- Document both modes (manual vs auto) in README

## Capabilities

### New Capabilities
- `auto-build`: IFD-based `buildFromUnitGraphAuto` that generates the build plan at eval time with no manual step, using vendored crate sources from `Cargo.lock` checksums
- `cargo-vendor`: Nix-native vendoring of crate sources from `Cargo.lock`, producing a cargo-compatible vendor directory for sandboxed plan generation

### Modified Capabilities

## Impact

- New files: `lib/auto.nix` (IFD orchestration), `lib/vendor.nix` (cargo vendoring)
- `flake.nix`: new lib output `buildFromUnitGraphAuto`
- `README.md`: document auto vs manual modes
- Existing `buildFromUnitGraph` unchanged — no breaking changes
