## Why

Cross-compilation support exists in unit2nix (CLI `--target` flag, Nix target mismatch warning, proc-macro/build-dep routing to build platform) but has zero test coverage. The entire code path is untested — a regression could silently break cross builds with no signal. Adding end-to-end cross-compilation tests validates the existing machinery and prevents future regressions.

## What Changes

- Add a cross-compilation nix flake check that generates a build plan with `--target aarch64-unknown-linux-gnu` and builds it with `pkgsCross.aarch64-multiplatform`
- Use the sample workspace (pure Rust, proc-macro, build script) — covers the key cross-compilation concerns without needing external -sys crate overrides
- Validate that proc-macros and build scripts execute on the build platform while library/binary crates target aarch64
- Validate that the target mismatch warning fires correctly (and doesn't fire when target matches)
- Add a Rust unit test for the `--target` flag flowing through to the JSON output

## Capabilities

### New Capabilities
- `cross-compilation-validation`: End-to-end test that cross-compilation works — generates an aarch64 build plan from the sample workspace, builds it with cross pkgs, and validates the output binaries are the correct architecture

### Modified Capabilities

## Impact

- `nix/checks.nix`: New cross-compilation check entries
- `sample_workspace/`: May need a cross-specific build plan JSON checked in (or generated via IFD in the test)
- `src/merge.rs` or `src/cargo.rs`: New unit test for target propagation
- CI time increases slightly (cross builds are slower than native)
