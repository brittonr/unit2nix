# Workspace Filtering — Design

## Overview

Two-layer filtering: Rust CLI filters at plan-generation time (controls what goes into the JSON), Nix consumer filters at eval time (controls what gets built from an existing JSON).

## CLI: `--members`

### Flag

```
--members <name1,name2,...>
```

Comma-separated list of workspace member names (as they appear in `cargo metadata`'s `workspace_members` field, e.g., `fd-find`, `nu-command`).

### Behavior

1. `cargo build --unit-graph` and `cargo metadata` still run for the full workspace (needed for correct dependency resolution — cargo does the resolution, not us)
2. After merge, filter:
   - `plan.roots` → only roots whose package IDs match selected members
   - `plan.workspace_members` → only entries for selected members
3. All crates in the dependency graph remain in `plan.crates` (they're needed as transitive deps) — only the "entry points" change
4. When `--members` is combined with `--package`, error out (conflicting semantics)
5. When a member name doesn't match any workspace member, emit a clear error listing valid members

### Why not filter `plan.crates`?

Removing crates from the plan would require computing the transitive closure of selected members' dependencies — duplicating work cargo already did. Keeping all crates is harmless (unused ones are never built by Nix due to laziness) and avoids correctness risks.

## Nix: `members` parameter

### Parameter

```nix
buildFromUnitGraph {
  # ...
  members = [ "my-bin" "my-lib" ];  # default: null (all members)
}
```

### Behavior

1. When `members` is non-null, filter `workspaceMembers` to only include listed names
2. `allWorkspaceMembers` only includes filtered members
3. `rootCrate` still uses the first root from the JSON (unchanged)
4. `test.workspaceMembers` and `clippy.workspaceMembers` also filtered
5. Invalid member names → `builtins.throw` with available members listed

### Why both CLI and Nix filtering?

- CLI filtering is permanent (baked into JSON) — good for projects that never build certain members
- Nix filtering is dynamic (same JSON, different views) — good for CI matrix jobs, dev vs prod builds
- They compose: CLI can pre-filter, then Nix can further narrow

## Validation

- `--members fd-find` on the fd workspace (single member, should be no-op)
- `--members nu` on nushell workspace (1 of 29 members)
- Nix `members = ["sample-bin"]` on sample_workspace (2 of 4 members excluded)
- Error case: `--members nonexistent` shows valid members
- Error case: `--members foo --package bar` errors
