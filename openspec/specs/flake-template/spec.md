# flake-template Specification

## Purpose
TBD - created by syncing archived change 2026-03-01-flake-template-staleness-validation. Update Purpose after archive sync.
## Requirements
### Requirement: Flake template scaffolds a working unit2nix project
The system SHALL provide a flake template at `templates/default/` that is registered in `flake.nix` outputs. Running `nix flake init -t <unit2nix-flake>` SHALL create a `flake.nix` pre-wired with unit2nix's `buildFromUnitGraph` function.

#### Scenario: User initializes a new project with the template
- **WHEN** a user runs `nix flake init -t github:brittonr/unit2nix` in a directory containing `Cargo.toml` and `Cargo.lock`
- **THEN** a `flake.nix` is created that imports unit2nix and calls `buildFromUnitGraph` with `src = ./.` and `resolvedJson = ./build-plan.json`

#### Scenario: Template includes generation instructions
- **WHEN** a user opens the generated `flake.nix`
- **THEN** comments in the file SHALL explain how to generate `build-plan.json` using `unit2nix --manifest-path ./Cargo.toml -o build-plan.json`

### Requirement: Template flake evaluates without error when build-plan.json exists
The generated `flake.nix` SHALL evaluate successfully with `nix eval` when a valid `build-plan.json` is present alongside `Cargo.toml` and `Cargo.lock`.

#### Scenario: Template evaluates with valid build plan
- **WHEN** the user has generated `build-plan.json` for their project
- **THEN** `nix eval .#packages.x86_64-linux.default` SHALL succeed without error

### Requirement: Template includes .gitignore
The template SHALL include a `.gitignore` file that ignores `result` and `target/` directories.

#### Scenario: Nix build artifacts are ignored
- **WHEN** the template is initialized
- **THEN** the `.gitignore` file SHALL contain entries for `result` and `/target`
