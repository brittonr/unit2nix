## ADDED Requirements

### Requirement: unit2nix generates a valid build plan for ripgrep
The unit2nix CLI SHALL successfully produce a `build-plan.json` for the ripgrep workspace without errors or dangling dependency references.

#### Scenario: Build plan generation succeeds
- **WHEN** `unit2nix --manifest-path <ripgrep>/Cargo.toml -o build-plan.json` is run against a checkout of ripgrep
- **THEN** the command SHALL exit 0 and produce valid JSON with no dangling dependency errors

### Requirement: ripgrep builds from the generated plan
The Nix consumer SHALL successfully build the ripgrep binary from the generated `build-plan.json` using `buildFromUnitGraph`.

#### Scenario: ripgrep binary builds and runs
- **WHEN** the generated `build-plan.json` is consumed by `buildFromUnitGraph` with appropriate `-sys` crate overrides
- **THEN** `nix build` SHALL succeed and the resulting binary SHALL execute `rg --version` without error

### Requirement: Discovered issues are fixed
Any bugs or edge cases discovered during ripgrep validation SHALL be fixed in the same change. Fixes SHALL be covered by unit tests.

#### Scenario: New edge case found and fixed
- **WHEN** ripgrep's build plan reveals a previously unhandled pattern (e.g., new source type, unusual crate configuration)
- **THEN** the fix SHALL include a corresponding unit test in `src/main.rs`
