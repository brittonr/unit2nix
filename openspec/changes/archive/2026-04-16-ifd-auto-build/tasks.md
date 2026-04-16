## 1. Vendor module

- [x] 1.1 Create `lib/vendor.nix` that parses `Cargo.lock` via `lib.importTOML` and classifies packages by source type
- [x] 1.2 Implement crates.io fetching: `fetchurl` with checksum from `Cargo.lock`, unpack with `.cargo-checksum.json`
- [x] 1.3 Implement git source fetching: `fetchgit` from `crate-hashes.json`, fallback to `builtins.fetchGit`
- [x] 1.4 Implement `linkFarm` vendor directory and cargo config generation (crates-io + git source redirects)

## 2. Auto-build function

- [x] 2.1 Create `lib/auto.nix` that takes `{ pkgs, src, ... }`, calls vendor, runs unit2nix in `mkDerivation`, and IFDs the result into `buildFromUnitGraph`
- [x] 2.2 Forward `defaultCrateOverrides`, `buildRustCrateForPkgs`, and other optional args to `buildFromUnitGraph`
- [x] 2.3 Handle optional `crate-hashes.json` loading from `src`

## 3. Flake integration

- [x] 3.1 Expose `buildFromUnitGraphAuto` in `flake.nix` lib output
- [x] 3.2 Add a flake check that builds `sample_workspace` via auto mode

## 4. Validation

- [x] 4.1 Verify `buildFromUnitGraphAuto` builds `sample_workspace` successfully
- [x] 4.2 Verify output matches manual `buildFromUnitGraph` (same binaries produced)
- [x] 4.3 Verify `nix flake check` passes with auto-mode check included

## 5. Documentation

- [x] 5.1 Add auto mode section to README with usage example
- [x] 5.2 Document trade-offs: auto (IFD, no Hydra) vs manual (checked-in JSON, works everywhere)
- [x] 5.3 Document `crate-hashes.json` for git deps
