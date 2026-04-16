## ADDED Requirements

### Requirement: Build workspace from JSON
The Nix consumer SHALL accept a pre-resolved JSON file (produced by unit2nix) and a source path, and produce `buildRustCrate` derivations for every crate in the build plan.

#### Scenario: Build a workspace with lib and bin crates
- **WHEN** `buildFromUnitGraph { src = ./.; resolvedJson = ./build-plan.json; }` is evaluated
- **THEN** the result MUST contain `workspaceMembers` with a `.build` derivation for each workspace member

#### Scenario: Root crate shortcut
- **WHEN** the JSON has a non-null `roots` field
- **THEN** `rootCrate.build` MUST be a valid derivation of the root package

### Requirement: Fetch crates.io sources
The consumer SHALL fetch crates.io dependencies using `fetchurl` with the SHA256 hash from the JSON.

#### Scenario: crates.io dependency with SHA256
- **WHEN** a crate has `source.type = "crates-io"` and a `sha256` field
- **THEN** the source MUST be fetched from `https://static.crates.io/crates/{name}/{name}-{version}.crate`
- **AND** the SHA256 hash MUST match the value in the JSON

### Requirement: Fetch git sources
The consumer SHALL fetch git dependencies using `builtins.fetchGit`.

#### Scenario: Git dependency with rev
- **WHEN** a crate has `source.type = "git"` with `url` and `rev` fields
- **THEN** the source MUST be fetched using `builtins.fetchGit { url; rev; }`

### Requirement: Resolve local path sources
The consumer SHALL resolve local path dependencies relative to the workspace source root.

#### Scenario: Local workspace member
- **WHEN** a crate has `source.type = "local"` with `path = "crates/foo"`
- **THEN** the source MUST be `src + "/crates/foo"`

#### Scenario: Workspace root crate
- **WHEN** a crate has `source.type = "local"` with `path = "."`
- **THEN** the source MUST be `src` (the workspace root)

### Requirement: Wire dependencies correctly
The consumer SHALL distinguish normal dependencies from build dependencies and wire them to the correct `buildRustCrate` arguments.

#### Scenario: Normal dependency
- **WHEN** a crate lists a dependency in `dependencies`
- **THEN** the dependency's derivation MUST appear in `buildRustCrate`'s `dependencies` argument

#### Scenario: Build dependency
- **WHEN** a crate lists a dependency in `buildDependencies`
- **THEN** the dependency's derivation MUST appear in `buildRustCrate`'s `buildDependencies` argument

### Requirement: Handle proc-macro crates
The consumer SHALL build proc-macro crates for the host platform.

#### Scenario: Proc-macro dependency
- **WHEN** a crate has `procMacro = true`
- **THEN** `buildRustCrate` MUST receive `procMacro = true`
- **AND** the crate MUST be built for the host platform (using `buildPackages`)

### Requirement: Handle crate renames
The consumer SHALL pass crate renames to `buildRustCrate` via `crateRenames`.

#### Scenario: Renamed dependency
- **WHEN** a dependency has `externCrateName` different from the target crate's `crateName`
- **THEN** `crateRenames` MUST include an entry mapping the original name to the renamed extern crate name

### Requirement: Pass resolved features
The consumer SHALL pass the pre-resolved feature list to each `buildRustCrate` invocation.

#### Scenario: Features from unit graph
- **WHEN** a crate has `features = ["default", "std", "derive"]`
- **THEN** `buildRustCrate` MUST receive `features = ["default" "std" "derive"]`

### Requirement: Expose as flake library
The flake MUST export `lib.buildFromUnitGraph` so other flakes can consume unit2nix output.

#### Scenario: Import from another flake
- **WHEN** a downstream flake has `unit2nix` as an input
- **THEN** `unit2nix.lib.${system}.buildFromUnitGraph { pkgs; src; resolvedJson; }` MUST return a valid workspace build result
