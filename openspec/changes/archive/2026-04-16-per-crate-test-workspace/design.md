# Design: Per-Crate Test Support via `--workspace`

## Approach

### Rust CLI

1. **New `--workspace` flag** in `cli.rs` — boolean flag
2. **`append_common_args`** in `cargo.rs` — when workspace is set, add `--workspace` to cargo args
3. **Implied `--include-dev`** in `run.rs` — when `--workspace` is set, always run the test unit graph
4. **Fingerprint update** — `--workspace` flag value included in inputs hash

### Nix Consumer

No changes needed to `build-from-unit-graph.nix` — it already handles `devDependencies` generically for any workspace member that has them. The fix is upstream: generating the right data.

### Auto Mode

1. **`auto.nix`**: Pass `--workspace` when `includeTests` or `workspace` param is set
2. **`flake.nix`**: Wire `workspace` param through `buildFromUnitGraphAuto`

### Validation

1. Add dev-deps to `sample-bin` (e.g. `assert_cmd`) so there are now TWO workspace members with dev-deps
2. Regenerate `build-plan.json` with `--workspace` — verify both members' dev-deps appear
3. `test.check.sample-lib` and `test.check.sample-bin` both pass

## Constraints

- `--workspace` and `--package` are mutually exclusive (cargo enforces this)
- `--workspace` implies `--include-dev` (simplifies UX — the flag's purpose is per-crate tests)
- Backward compatible: old build plans without `--workspace` continue to work
