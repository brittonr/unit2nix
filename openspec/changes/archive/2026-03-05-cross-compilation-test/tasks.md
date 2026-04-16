## 1. Rust unit test for target propagation

- [x] 1.1 Add unit test in `src/merge.rs` that calls `merge()` with `target = Some("aarch64-unknown-linux-gnu")` and asserts `plan.target == Some("aarch64-unknown-linux-gnu")`
- [x] 1.2 Run `cargo test` — confirm 48/48 pass (47 existing + 1 new)

## 2. Cross-compilation build plan generation

- [x] 2.1 Create `tests/cross/` directory for cross-compilation test artifacts
- [x] 2.2 Write `tests/cross/build.nix` that uses IFD to generate an aarch64 build plan from the sample workspace and builds it with `pkgsCross.aarch64-multiplatform`
- [x] 2.3 In `build.nix`, validate the output binary architecture — use `file` to assert `sample-bin` is `ELF 64-bit` `ARM aarch64`

## 3. Flake check integration

- [x] 3.1 Add `validate-cross-aarch64` check to `nix/checks.nix` that imports `tests/cross/build.nix`
- [x] 3.2 Gate the check behind `pkgs.stdenv.isx86_64` (cross builds only make sense from x86_64)
- [x] 3.3 Run `nix flake check` — confirm the new check passes alongside all existing checks

## 4. Documentation and napkin

- [x] 4.1 Update README tested projects table or cross-compilation section to note that cross builds are validated in CI
- [x] 4.2 Update napkin with session notes and lessons learned
