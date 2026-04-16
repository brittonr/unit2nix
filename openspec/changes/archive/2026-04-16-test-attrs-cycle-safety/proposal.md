## Why

`9fcd411` fixed infinite recursion for `test.check.<name>` by switching it to a per-member test graph, but `test.workspaceMembers` and `test.allWorkspaceMembers` still rely on the legacy shared `mkTestBuiltByPkgs` fixpoint. That leaves a live correctness hole for workspaces whose dev-dependency edges form cycles through normal dependencies.

## What Changes

- Rewire `test.workspaceMembers.<name>.build` to use per-member test graphs instead of the shared all-members test graph.
- Rewire `test.allWorkspaceMembers` to aggregate per-member test builds rather than forcing every workspace member into one shared dev-dependency fixpoint.
- Keep `test.check.<name>` aligned with the same per-member graph construction so all public test attrs share one cycle-safe model.
- Add a regression fixture/check for a workspace where a dev-dependency cycle would recurse under the legacy shared graph.
- Update docs/comments to describe the cycle-safe behavior of public test attrs.

## Capabilities

### New Capabilities
- `workspace-test-attrs`: Public Nix test attrs for workspace members remain evaluable even when dev-dependencies form cycles across workspace members.

### Modified Capabilities
- None.

## Impact

- **Files**: `lib/build-from-unit-graph.nix`, `nix/checks.nix`, regression fixture/check under `tests/`, and README/docs comments for test attrs.
- **APIs**: Public Nix attrs `test.workspaceMembers`, `test.allWorkspaceMembers`, and `test.check`.
- **Dependencies**: None.
- **Testing**: Add a targeted cyclic-workspace regression check and re-run relevant existing Nix test checks.
