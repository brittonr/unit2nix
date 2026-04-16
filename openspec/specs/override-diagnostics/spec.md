# override-diagnostics Specification

## Purpose
TBD - created by archiving change sys-crate-override-ergonomics. Update Purpose after archive.
## Requirements
### Requirement: Eval-time trace warnings for unmatched links
During crate set construction in `build-from-unit-graph.nix`, for each crate that has a `links` field and whose `crateName` is NOT present in the merged override set (nixpkgs + built-ins + user) and NOT in the `knownNoOverride` set, the system SHALL emit a `builtins.trace` warning.

The warning message MUST include the crate name, the `links` value, and a pointer to the sys-crate-overrides documentation.

#### Scenario: Unknown sys crate triggers trace warning
- **WHEN** a build plan includes `exotic-sys` with `links = "exotic"` and no override is configured for it
- **THEN** `builtins.trace` emits: `unit2nix: WARNING — crate 'exotic-sys' has links="exotic" but no override found. See docs/sys-crate-overrides.md`

#### Scenario: Covered sys crate does not trigger warning
- **WHEN** a build plan includes `libz-sys` with `links = "z"` and the built-in registry covers it
- **THEN** no warning is emitted for `libz-sys`

#### Scenario: Warnings do not fail the build
- **WHEN** a build plan includes an uncovered sys crate
- **THEN** evaluation continues normally — the trace warning is advisory only

### Requirement: CLI --check-overrides flag
The `unit2nix` CLI SHALL accept a `--check-overrides` flag that reads a build plan JSON file and reports override coverage.

The report MUST list:
1. All crates with `links` fields
2. Which of those have known overrides (from a compiled-in registry)
3. Which are missing overrides, with a human-readable note about what the crate typically needs

#### Scenario: Check overrides on bat build plan
- **WHEN** `unit2nix --check-overrides -o build-plan.json` is run against the bat build plan
- **THEN** the output lists `libgit2-sys` (covered), `libz-sys` (covered), `onig_sys` (covered), `rayon-core` (no override needed), `prettyplease02` (no override needed)

#### Scenario: Check overrides reports missing crate
- **WHEN** a build plan includes `unknown-sys` with `links = "unknown"` and it is not in the known registry
- **THEN** the output marks `unknown-sys` as "missing" with a note: "needs native library — add to extraCrateOverrides"

#### Scenario: Check overrides with no links crates
- **WHEN** a build plan has no crates with `links` fields (pure Rust)
- **THEN** the output says "No crates with native link requirements found"

### Requirement: Updated documentation
`docs/sys-crate-overrides.md` SHALL be updated to:
1. Lead with "most common -sys crates work out of the box"
2. Document the three-layer override hierarchy
3. Document `extraCrateOverrides` as the recommended parameter for project-specific overrides
4. Document `--check-overrides` usage
5. Retain the recipe reference for manual overrides

#### Scenario: Documentation reflects new defaults
- **WHEN** a user reads `docs/sys-crate-overrides.md`
- **THEN** the first section explains that built-in overrides cover common cases, followed by how to add project-specific ones via `extraCrateOverrides`

