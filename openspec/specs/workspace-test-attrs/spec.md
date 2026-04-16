# workspace-test-attrs Specification

## Purpose
TBD - created by archiving change test-attrs-cycle-safety. Update Purpose after archive.
## Requirements
### Requirement: Public workspace test attrs isolate dev-dependencies per member

The Nix consumer MUST build public workspace test attrs from per-member test graphs. Only the selected workspace member may receive its `devDependencies`; other workspace members in the dependency closure MUST resolve through their normal builds.

#### Scenario: Per-member test build stays isolated

- GIVEN a workspace member `foo` with dev-dependencies and another workspace member `bar` reachable through normal dependencies
- WHEN a user evaluates `test.workspaceMembers.foo.build`
- THEN `foo` is built from a test graph that includes its dev-dependencies
- AND `bar` resolves through the normal non-test build graph

#### Scenario: Test runner matches isolated build model

- GIVEN a workspace member `foo` with dev-dependencies
- WHEN a user evaluates `test.check.foo`
- THEN the test binary build uses the same per-member graph isolation as `test.workspaceMembers.foo.build`

### Requirement: Aggregate workspace test attr is cycle-safe

The Nix consumer MUST expose `test.allWorkspaceMembers` without evaluating a shared fixpoint that injects dev-dependencies for every workspace member simultaneously.

#### Scenario: Aggregate attr survives dev-dependency cycle

- GIVEN a workspace where member `a` has a dev-dependency edge that closes a cycle through other members' normal dependencies
- WHEN a user evaluates `test.allWorkspaceMembers`
- THEN evaluation succeeds without infinite recursion
- AND the result joins per-member test builds

#### Scenario: Regression is exercised in flake checks

- GIVEN a regression fixture for a cyclic workspace test graph
- WHEN repository checks run
- THEN `nix/checks.nix` evaluates the affected public test attrs for that fixture
- AND the regression is not manual-only

#### Scenario: Unrelated member is not poisoned by another member's cycle

- GIVEN a workspace where member `a` has a cyclic dev-dependency closure and member `b` does not
- WHEN a user evaluates `test.workspaceMembers.b.build`
- THEN evaluation succeeds without forcing `a` into the same shared dev-dependency fixpoint

