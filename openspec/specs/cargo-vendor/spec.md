# cargo-vendor Specification

## Purpose
TBD - created by archiving change ifd-auto-build. Update Purpose after archive.
## Requirements
### Requirement: Vendor crate sources from Cargo.lock
The vendor module SHALL parse `Cargo.lock` via `lib.importTOML` and produce a cargo-compatible vendor directory with all crate sources.

#### Scenario: crates.io dependencies
- **WHEN** `Cargo.lock` contains crates.io packages with checksums
- **THEN** each crate is fetched via `pkgs.fetchurl` using the checksum from `Cargo.lock` and unpacked into the vendor directory with a `.cargo-checksum.json` file

#### Scenario: Git dependencies with hashes
- **WHEN** `Cargo.lock` contains git dependencies and `crate-hashes.json` provides their SHA256 hashes
- **THEN** each git dep is fetched via `pkgs.fetchgit` using the provided hash

#### Scenario: Git dependencies without hashes
- **WHEN** `Cargo.lock` contains git dependencies and no hash is available in `crate-hashes.json`
- **THEN** the dep is fetched via `builtins.fetchGit` (which may require `--impure` in some Nix configurations)

### Requirement: Cargo config for vendored sources
The vendor module SHALL produce a cargo config file that redirects `[source.crates-io]` and any git sources to the vendor directory.

#### Scenario: Cargo uses vendored sources
- **WHEN** cargo is invoked with the generated config
- **THEN** cargo resolves all dependencies from the vendor directory with no network access

