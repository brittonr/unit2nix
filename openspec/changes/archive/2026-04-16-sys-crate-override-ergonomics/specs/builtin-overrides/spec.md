## ADDED Requirements

### Requirement: Built-in override registry
unit2nix SHALL ship a `lib/crate-overrides.nix` file that exports a function `{ pkgs }: attrset` containing `buildRustCrate`-compatible overrides for common -sys crates not covered by `pkgs.defaultCrateOverrides`.

The registry MUST include overrides for at minimum: `libsqlite3-sys`, `ring`, `tikv-jemalloc-sys`, `onig_sys`, and `prost-build` (crates validated in unit2nix's test suite that currently require user overrides).

Each override MUST provide the correct `nativeBuildInputs`, `buildInputs`, and environment variables for the crate to build in a Nix sandbox.

#### Scenario: bat builds without user overrides for libgit2-sys and libz-sys
- **WHEN** `buildFromUnitGraph` is called with no `defaultCrateOverrides` or `extraCrateOverrides` for the bat build plan
- **THEN** bat builds successfully because `libgit2-sys` and `libz-sys` are in the built-in registry

#### Scenario: nushell builds with minimal user overrides
- **WHEN** `buildFromUnitGraph` is called with no user overrides for the nushell build plan
- **THEN** nushell builds successfully because `libsqlite3-sys` and `ring` are in the built-in registry

### Requirement: Three-layer override merging
`buildFromUnitGraph` SHALL merge overrides in this order: `pkgs.defaultCrateOverrides` → unit2nix built-in overrides → user `extraCrateOverrides`. Later layers override earlier ones for the same crate name.

#### Scenario: User extraCrateOverrides wins over built-in
- **WHEN** the built-in registry provides an override for `libz-sys` AND the user passes `extraCrateOverrides = { libz-sys = attrs: { CUSTOM = "1"; }; }`
- **THEN** the user's `libz-sys` override is used, not the built-in one

#### Scenario: defaultCrateOverrides replaces entire base layer
- **WHEN** the user passes `defaultCrateOverrides = { only-mine = attrs: { }; }`
- **THEN** neither `pkgs.defaultCrateOverrides` nor unit2nix built-ins are applied — only `only-mine` is in the override set

### Requirement: extraCrateOverrides parameter
`buildFromUnitGraph` SHALL accept an optional `extraCrateOverrides` parameter (default: `{}`). This attrset is merged on top of the default override stack (nixpkgs + unit2nix built-ins).

The existing `defaultCrateOverrides` parameter SHALL continue to work unchanged for backward compatibility.

#### Scenario: Only project-specific overrides needed
- **WHEN** a project has a custom -sys crate `my-sys` not in any default set
- **THEN** the user passes `extraCrateOverrides = { my-sys = attrs: { buildInputs = [ pkgs.mylib ]; }; }` and it merges on top of all defaults

#### Scenario: No parameters needed for pure-Rust projects
- **WHEN** a project has no -sys crates requiring native libraries
- **THEN** `buildFromUnitGraph` works with no override parameters at all (current behavior preserved)

### Requirement: Known no-override suppression set
The built-in registry SHALL include a `knownNoOverride` set of crate names whose `links` field is Rust-internal and never requires native library overrides (e.g., `rayon-core`, `prettyplease02`).

#### Scenario: rayon-core does not trigger a warning
- **WHEN** a build plan includes `rayon-core` with `links = "rayon-core"`
- **THEN** no missing-override warning is emitted because `rayon-core` is in the `knownNoOverride` set
