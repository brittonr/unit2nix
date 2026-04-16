# auto-build Specification

## Purpose
TBD - created by archiving change ifd-auto-build. Update Purpose after archive.
## Requirements
### Requirement: Auto-build with no manual step
`buildFromUnitGraphAuto` SHALL generate the build plan at eval time via IFD and produce the same result as manual `buildFromUnitGraph`.

#### Scenario: Basic usage
- **WHEN** a user calls `buildFromUnitGraphAuto { pkgs; src = ./.; }`
- **THEN** the workspace is built using per-crate `buildRustCrate` derivations with no `resolvedJson` argument and no pre-generated `build-plan.json`

#### Scenario: Same output as manual mode
- **WHEN** a workspace is built with `buildFromUnitGraphAuto`
- **THEN** the resulting derivations SHALL be functionally equivalent to building with `buildFromUnitGraph` using a manually generated `build-plan.json`

#### Scenario: Pure eval compatible
- **WHEN** `nix build` or `nix flake check` is run with `--pure-eval`
- **THEN** `buildFromUnitGraphAuto` succeeds (IFD is orthogonal to pure eval)

### Requirement: Forward all buildFromUnitGraph options
`buildFromUnitGraphAuto` SHALL accept and forward `buildRustCrateForPkgs`, `defaultCrateOverrides`, and any other optional args supported by `buildFromUnitGraph`.

#### Scenario: Crate overrides
- **WHEN** a user passes `defaultCrateOverrides` to `buildFromUnitGraphAuto`
- **THEN** the overrides are applied to the built crates, identical to manual mode

### Requirement: Optional crate-hashes.json for git deps
`buildFromUnitGraphAuto` SHALL read `crate-hashes.json` from the workspace root if present, using it to fetch git dependencies as pure fixed-output derivations.

#### Scenario: Git deps with crate-hashes.json
- **WHEN** the workspace has git dependencies and `src` contains a `crate-hashes.json`
- **THEN** git deps are fetched via `pkgs.fetchgit` with the provided hashes (pure, no `--impure` needed)

#### Scenario: No git deps
- **WHEN** the workspace has only crates.io and local dependencies
- **THEN** `buildFromUnitGraphAuto` works with no `crate-hashes.json` and no `--impure`

