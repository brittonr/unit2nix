# sample-workspace Specification

## Purpose
TBD - created by syncing archived change 2026-03-01-nix-build-consumer. Update Purpose after archive sync.
## Requirements
### Requirement: Multi-crate sample workspace
The sample workspace SHALL contain at least 4 crates covering the common Cargo patterns that `buildRustCrate` must handle.

#### Scenario: Library crate
- **WHEN** the workspace contains a library crate
- **THEN** `nix build .#sample.workspaceMembers.sample-lib.build` MUST produce a lib output

#### Scenario: Binary crate with lib dependency
- **WHEN** the workspace contains a binary crate depending on the library crate
- **THEN** `nix build .#sample.workspaceMembers.sample-bin.build` MUST produce an executable
- **AND** the executable MUST link against the library

#### Scenario: Proc-macro crate
- **WHEN** the workspace contains a proc-macro crate
- **THEN** it MUST be built for the host platform
- **AND** the binary crate MUST be able to use its derive macro

#### Scenario: Build script crate
- **WHEN** a crate in the workspace has a `build.rs`
- **THEN** the build script MUST execute during the build
- **AND** environment variables set by the build script MUST be available to the crate

### Requirement: Pre-generated build plan
The sample workspace SHALL include a pre-generated `build-plan.json` so Nix builds work without the Rust toolchain at eval time.

#### Scenario: Nix build without cargo
- **WHEN** `nix build .#sample` is run in an environment without `cargo`
- **THEN** the build MUST succeed using only the pre-generated JSON

### Requirement: External crates.io dependency
The sample workspace SHALL depend on at least one crates.io crate to validate source fetching and SHA256 verification.

#### Scenario: crates.io dependency builds
- **WHEN** the sample workspace depends on a crates.io crate (e.g., `serde`)
- **THEN** the crate MUST be fetched, unpacked, and compiled via `buildRustCrate`

### Requirement: Feature flags
At least one crate in the sample workspace SHALL use feature flags to validate that resolved features are passed correctly.

#### Scenario: Feature-gated code compiles
- **WHEN** a crate has `features = ["serde"]` in the build plan
- **THEN** code behind `#[cfg(feature = "serde")]` MUST compile
- **AND** code behind `#[cfg(feature = "disabled-feature")]` MUST NOT compile
