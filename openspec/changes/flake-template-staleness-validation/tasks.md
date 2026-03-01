## 1. Staleness Check — Rust CLI

- [x] 1.1 Add SHA256 hashing of Cargo.lock content to `src/main.rs` (use `sha2` crate or `std` — compute hex-encoded SHA256 of the raw file bytes)
- [x] 1.2 Add `cargoLockHash` field to the `NixBuildPlan` struct and include it in JSON output
- [x] 1.3 Add unit test: `cargoLockHash` is present and is a 64-char hex string
- [x] 1.4 Verify `cargo test` passes with the new field

## 2. Staleness Check — Nix Consumer

- [x] 2.1 Add `skipStalenessCheck` parameter to `buildFromUnitGraph` in `lib/build-from-unit-graph.nix` (default `false`)
- [x] 2.2 Add eval-time assertion: compare `resolved.cargoLockHash` with `builtins.hashFile "sha256" (src + "/Cargo.lock")` when hash is present and check is not skipped
- [x] 2.3 Error message must include "build-plan.json is out of date" and the `unit2nix` regeneration command
- [x] 2.4 Ensure backwards compatibility: skip check when `cargoLockHash` is absent from JSON
- [x] 2.5 Update `flake.nix` to pass `skipStalenessCheck` through the library wrapper
- [x] 2.6 Regenerate sample_workspace `build-plan.json` with the new `cargoLockHash` field
- [x] 2.7 Verify `nix flake check` passes with the staleness check active

## 3. Flake Template

- [x] 3.1 Create `templates/default/flake.nix` with unit2nix input, `buildFromUnitGraph` call, and generation instructions in comments
- [x] 3.2 Create `templates/default/.gitignore` with `result` and `/target` entries
- [x] 3.3 Register the template in `flake.nix` outputs under `templates.default`
- [x] 3.4 Verify `nix flake init -t .` works in a temp directory (creates expected files)

## 4. Real-World Validation — ripgrep

- [x] 4.1 Clone ripgrep, generate `build-plan.json` with `unit2nix`, fix any CLI errors
- [x] 4.2 Wire up a Nix expression that builds ripgrep via `buildFromUnitGraph` with necessary `-sys` overrides
- [x] 4.3 Verify the built `rg` binary runs (`rg --version`)
- [x] 4.4 Add unit tests for any new edge cases discovered (none found — clean build)
- [x] 4.5 Document required overrides (if any) in README or as a comment in the test (none needed — pure Rust)

## 5. README & Cleanup

- [x] 5.1 Update README quickstart to reference `nix flake init -t github:brittonr/unit2nix`
- [x] 5.2 Document the staleness check and `skipStalenessCheck` option in the Nix API section
- [x] 5.3 Mention ripgrep validation in the README (tested projects list)
