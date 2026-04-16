## 1. CLI `--workspace` flag

- [x] 1.1 Add `--workspace` flag to `src/cli.rs` — boolean, doc comment explaining it passes `--workspace` to cargo and implies `--include-dev`
- [x] 1.2 Validate: error if both `--workspace` and `--package` are specified (in `run.rs`)

## 2. Cargo invocations

- [x] 2.1 Add `--workspace` to `append_common_args` in `cargo.rs`
- [x] 2.2 When workspace is set, add `--workspace` to `cargo build --unit-graph` args
- [x] 2.3 When workspace is set, add `--workspace` to `cargo test --unit-graph` args
- [x] 2.4 When `--workspace` is set, always run the test unit graph (imply `--include-dev`)

## 3. Fingerprint

- [x] 3.1 Include `workspace` flag value in `compute_inputs_hash`

## 4. Auto mode

- [x] 4.1 Add `workspace ? false` parameter to `auto.nix`
- [x] 4.2 Pass `--workspace` to unit2nix invocation in IFD derivation when set
- [x] 4.3 Wire `workspace` param through `buildFromUnitGraphAuto` in `flake.nix`

## 5. Sample workspace validation

- [x] 5.1 Add dev-dependency (`pretty_assertions`) to `sample-bin`
- [x] 5.2 Add a test to `sample-bin/src/main.rs` that uses the dev-dep
- [x] 5.3 Regenerate `sample_workspace/build-plan.json` with `--workspace`
- [x] 5.4 Verify both `sample-lib` and `sample-bin` have `devDependencies` in the build plan

## 6. Tests

- [x] 6.1 Unit test: fingerprint changes with `--workspace` flag (new test in fingerprint.rs)
- [x] 6.2 Nix check: `sample-run-tests` (sample-lib) passes
- [x] 6.3 Nix check: `sample-run-tests-bin` (sample-bin) passes with dev-dep
- [x] 6.4 All 18 Nix checks pass, 59 Rust unit tests pass

## 7. Documentation

- [x] 7.1 Update CLI `--help` (clap doc comment handles this)
- [x] 7.2 Update README with `--workspace` flag docs
- [x] 7.3 Update napkin with session notes
