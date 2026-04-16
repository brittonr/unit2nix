# Per-Crate Test Support via `--workspace` Mode

## Problem

The `--include-dev` flag runs `cargo test --unit-graph` to capture dev-dependencies, but **without `--workspace`**. This means only the default workspace member(s)' dev-deps are captured — other members' dev-deps are silently missing from the build plan.

For a workspace like:
```
workspace/
  crate-a/  (dev-dep: pretty_assertions)
  crate-b/  (dev-dep: proptest)
  crate-c/  (dev-dep: tempfile)
```

Running `unit2nix --include-dev` only captures crate-a's dev-deps (the default member). `test.check.crate-b` and `test.check.crate-c` fail because `proptest` and `tempfile` aren't in the build plan.

Similarly, `cargo build --unit-graph` without `--workspace` may miss non-default workspace members entirely.

## Solution

Add `--workspace` CLI flag that passes `--workspace` to both `cargo build --unit-graph` and `cargo test --unit-graph`, ensuring ALL workspace members and ALL their dev-deps are captured.

When `--workspace` is set, `--include-dev` is implied — the whole point is per-crate test support across the full workspace.

## Value

- **Per-crate tests**: `test.check.<any-member>` works for every workspace member
- **Correctness**: All workspace members' dev-deps are captured, not just the default member's
- **Auto mode**: `buildFromUnitGraphAuto` can enable workspace mode for full test coverage
