# Workspace Filtering

## Problem

Large Rust workspaces (e.g., nushell with 29 workspace members) build *all* members by default. Users often only want to build a subset — e.g., just the main binary, not the benchmarks or internal test crates. Currently the only filtering option is `--package` which selects a single package for `cargo build --unit-graph`, but:

1. `--package` only accepts one crate, not a subset of workspace members
2. The Nix consumer has no way to select members — `allWorkspaceMembers` builds everything
3. There's no way to exclude members (e.g., "everything except bench-tools")

## Solution

Add `--members` CLI flag (comma-separated list of workspace member names) that filters which members appear in `workspace_members` and `roots` in the build plan JSON. The full dependency graph is still resolved (needed for correctness), but only the selected members are exposed as roots/workspace-members.

On the Nix side, add an optional `members` parameter to `buildFromUnitGraph` that filters `workspaceMembers` at eval time (for users who want to filter without regenerating the plan).

## Value

- **Build speed**: Only build what you need — skip irrelevant workspace members and their unique deps
- **CI flexibility**: Different CI jobs can build different subsets without separate build plans
- **Nix eval-time filtering**: No regeneration needed to change which members are built
