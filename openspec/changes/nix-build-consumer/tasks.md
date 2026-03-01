## 1. Sample Workspace

- [ ] 1.1 Create `sample_workspace/` with root `Cargo.toml` workspace manifest
- [ ] 1.2 Add `sample-lib` crate: library with a public function and optional `serde` feature (derives `Serialize`/`Deserialize` behind `#[cfg(feature = "serde")]`)
- [ ] 1.3 Add `sample-macro` crate: proc-macro that derives a simple trait (e.g., `HelloMacro`)
- [ ] 1.4 Add `sample-bin` crate: binary that depends on `sample-lib` (with `serde` feature) and `sample-macro`, prints a message to verify linking works
- [ ] 1.5 Add `sample-build-script` crate: library with `build.rs` that sets a `GENERATED_VALUE` env var, lib code reads it via `env!("GENERATED_VALUE")`
- [ ] 1.6 Add one crates.io dependency (`serde`, `serde_json`) to validate external source fetching
- [ ] 1.7 Run `cargo build` in sample workspace to verify it compiles natively
- [ ] 1.8 Generate `sample_workspace/build-plan.json` using unit2nix

## 2. Nix Source Fetching

- [ ] 2.1 Create `lib/fetch-source.nix` — function that takes a crate's source info and returns a Nix source path
- [ ] 2.2 Handle `crates-io` type: `fetchurl` from `https://static.crates.io/crates/{name}/{name}-{version}.crate` with `sha256`
- [ ] 2.3 Handle `git` type: `builtins.fetchGit { url; rev; }`
- [ ] 2.4 Handle `local` type: `src + "/${path}"` (with `"."` → `src`)

## 3. Nix Consumer Core

- [ ] 3.1 Create `lib/build-from-unit-graph.nix` — main entry point accepting `{ pkgs, src, resolvedJson }`
- [ ] 3.2 Parse JSON via `builtins.fromJSON (builtins.readFile resolvedJson)`
- [ ] 3.3 Build self-referencing attrset: each crate derivation looks up deps by package ID
- [ ] 3.4 Wire `dependencies` → `buildRustCrate` `dependencies` argument (map packageId → derivation)
- [ ] 3.5 Wire `buildDependencies` → `buildRustCrate` `buildDependencies` argument
- [ ] 3.6 Handle proc-macro crates: use `self.build.crates.${depId}` (host platform) for proc-macro deps
- [ ] 3.7 Compute `crateRenames` from deps where `externCrateName` differs from target crate's `crateName`
- [ ] 3.8 Pass `features`, `edition`, `procMacro`, `crateBin`, `libPath`, `libName`, `links` to `buildRustCrate`
- [ ] 3.9 Return `{ workspaceMembers; rootCrate; allWorkspaceMembers; }` matching crate2nix's interface

## 4. Flake Integration

- [ ] 4.1 Add `lib.buildFromUnitGraph` to flake outputs
- [ ] 4.2 Add `packages.sample` that builds the sample workspace via `buildFromUnitGraph`
- [ ] 4.3 Add `checks.sample-builds` that verifies the sample workspace builds successfully

## 5. End-to-End Verification

- [ ] 5.1 Run `nix build .#sample` and verify it produces the sample binary
- [ ] 5.2 Execute the sample binary and verify it prints expected output
- [ ] 5.3 Verify proc-macro derive works in the built binary
- [ ] 5.4 Verify build script env var is baked into the built library
- [ ] 5.5 Run against aspen's `build-plan.json` to verify it handles a real 453-crate workspace (compilation check, not full build)
