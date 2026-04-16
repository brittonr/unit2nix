# update-plan-app Specification

## Purpose
TBD - created by archiving change update-plan-app. Update Purpose after archive.
## Requirements
### Requirement: Flake app regenerates build plan
The flake SHALL expose an `apps.<system>.update-plan` output that regenerates `build-plan.json` for the project.

#### Scenario: Fresh regeneration
- **WHEN** a user runs `nix run .#update-plan` from the project root
- **THEN** unit2nix runs against `./Cargo.toml` and writes the result to `./build-plan.json`

#### Scenario: Stale plan recovery
- **WHEN** `Cargo.lock` has changed and `nix build` fails with a staleness error
- **AND** the user runs `nix run .#update-plan`
- **THEN** `build-plan.json` is regenerated and subsequent `nix build` succeeds

### Requirement: Template includes update-plan app
The flake template SHALL include the `update-plan` app so new projects get it automatically.

#### Scenario: New project from template
- **WHEN** a user runs `nix flake init -t github:brittonr/unit2nix`
- **THEN** the generated `flake.nix` includes `apps.<system>.update-plan`

### Requirement: Staleness error suggests the fix
The staleness error in `build-from-unit-graph.nix` SHALL reference both `nix run .#update-plan` and `cargo unit2nix` as remediation options.

#### Scenario: Error message content
- **WHEN** `build-plan.json` is stale and `nix build` fails
- **THEN** the error message includes `nix run .#update-plan` as the primary command and `cargo unit2nix -o build-plan.json` as the secondary option

