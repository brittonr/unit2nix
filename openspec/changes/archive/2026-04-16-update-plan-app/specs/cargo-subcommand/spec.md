## ADDED Requirements

### Requirement: Cargo subcommand binary
The crate SHALL produce a `cargo-unit2nix` binary that Cargo auto-discovers as `cargo unit2nix`.

#### Scenario: Cargo subcommand invocation
- **WHEN** a user has `cargo-unit2nix` on PATH (via `cargo install` or Nix)
- **AND** runs `cargo unit2nix -o build-plan.json`
- **THEN** unit2nix generates a build plan for the workspace in the current directory

#### Scenario: cargo install
- **WHEN** a user runs `cargo install cargo-unit2nix`
- **THEN** both `unit2nix` and `cargo-unit2nix` binaries are installed

### Requirement: Identical behavior
The `cargo-unit2nix` binary SHALL accept the same arguments and produce the same output as the `unit2nix` binary.

#### Scenario: Same flags
- **WHEN** a user runs `cargo unit2nix --manifest-path ./Cargo.toml -o build-plan.json`
- **THEN** the output is identical to running `unit2nix --manifest-path ./Cargo.toml -o build-plan.json`
