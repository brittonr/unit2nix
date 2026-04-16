# Design: Cycle-Safe Workspace Test Attrs

## Context

`lib/build-from-unit-graph.nix` now has two test-graph paths:

1. `mkTestGraphForCrate` — per-member graph; only the selected member gets `devDependencies`
2. `mkTestBuiltByPkgs` — legacy shared graph; every workspace member with dev-deps is rebuilt together in one recursive fixpoint

`9fcd411` moved `test.check.<name>` to `mkTestGraphForCrate`, which fixes the most obvious recursion path. But `test.workspaceMembers.<name>.build` and `test.allWorkspaceMembers` still read from the shared graph via `testCrates`, so users can still hit infinite recursion by evaluating those attrs.

## Goals / Non-Goals

**Goals:**
- Make all public workspace test attrs use per-member graph isolation
- Preserve existing attr names and general output shape
- Add regression coverage for the dev-dep cycle that motivated `9fcd411`
- Reuse normal dependency builds wherever possible

**Non-Goals:**
- Redesign normal build or clippy graph construction
- Change CLI build-plan generation semantics
- Broaden test support beyond cycle-safety for current public attrs

## Decisions

### 1. Expose only per-member test graphs

**Choice:** Build `test.workspaceMembers.<name>.build` from `mkTestGraphForCrate {} pkgs packageId`, and build `test.allWorkspaceMembers` as a `symlinkJoin` over those per-member test builds.

**Rationale:** This matches `cargo test -p <member>` semantics and keeps dev-dependency expansion scoped to the selected member. Unrelated workspace members stay on normal builds, so a cycle in one member's dev-dep closure does not poison every exposed test attr.

**Alternative considered:** Keep the shared graph and try to break recursion inside the fixpoint. Rejected because it keeps unrelated members coupled and is harder to reason about.

### 2. Add a dedicated cyclic regression fixture and wire it into flake checks

**Choice:** Add a small workspace fixture/check whose graph includes a dev-dependency edge that closes a cycle through normal dependencies, then register it in `nix/checks.nix` so CI evaluates the affected public test attrs.

**Rationale:** `sample_workspace` covers normal behavior but does not exercise the failure mode. A purpose-built fixture prevents the bug from returning unnoticed, and wiring it into flake checks keeps it from becoming a manual-only test.

**Alternative considered:** Rely on comments or manual validation with an external repo. Rejected because the regression would not be exercised in CI.

### 3. Keep the public API stable

**Choice:** Preserve `test.workspaceMembers`, `test.allWorkspaceMembers`, and `test.check` as-is externally; only internal graph construction changes.

**Rationale:** Users already depend on these attr paths. This fix should tighten correctness, not force migration.

## Risks / Trade-offs

- **More per-member graph instantiations** → Acceptable. Non-target dependencies still reuse normal builds, and correctness is worth the modest eval overhead.
- **Regression fixture may be too synthetic** → Keep it minimal but shaped like the real failure: dev-dep edge plus normal dependency chain back to the origin.
- **Legacy helper becomes confusing dead weight** → If `mkTestBuiltByPkgs` / `testCrates` has zero remaining call sites after rewiring, delete it in the same change; only keep it if a non-public internal consumer still exists and document that consumer.

## Migration Plan

No user migration. Existing attr names stay stable.

Implementation order:
1. Rewire exposed test attrs to per-member graphs
2. Delete the legacy shared helper if no remaining consumer exists; otherwise document the retained internal-only use
3. Add regression fixture/check and register it in `nix/checks.nix`
4. Verify both the new cyclic check and the existing sample test checks remain green
5. Update docs/comments

## Open Questions

- None at proposal time.
