## 1. CLI `--members` flag

- [x] 1.1 Add `--members` flag to `src/cli.rs` — `Option<String>`, comma-separated workspace member names
- [x] 1.2 Validate: error if both `--members` and `--package` are specified
- [x] 1.3 Parse `--members` into `Vec<String>` in `run.rs`, pass to merge

## 2. Plan filtering in merge

- [x] 2.1 Add `members_filter: Option<&[String]>` parameter to `merge::merge()`
- [x] 2.2 When filter is set, validate all names exist in `metadata.workspace_members` — error with list of valid members if not
- [x] 2.3 Filter `plan.workspace_members` to only include selected names
- [x] 2.4 Filter `plan.roots` to only include package IDs of selected members
- [x] 2.5 Keep `plan.crates` unfiltered (transitive deps needed, Nix laziness handles the rest)

## 3. Dev-dep filtering

- [x] 3.1 Dev deps computed for all workspace members, filtering applied after — Nix laziness means filtered-out members' dev deps are never evaluated
- [x] 3.2 Filtering logs handled by eprintln in merge (workspace members count reflects filtered set)

## 4. Nix `members` parameter

- [x] 4.1 Add `members ? null` parameter to `buildFromUnitGraph`
- [x] 4.2 Compute `filteredWorkspaceMembers` — when `members` is non-null, `lib.filterAttrs (name: _: lib.elem name members)` on `resolved.workspaceMembers`
- [x] 4.3 Validate: `builtins.throw` if any name in `members` is not in `resolved.workspaceMembers`, listing valid names
- [x] 4.4 Replace all uses of `resolved.workspaceMembers` in output attrset with `filteredWorkspaceMembers`
- [x] 4.5 Wire through to `test.workspaceMembers`, `test.check`, `clippy.workspaceMembers`
- [x] 4.6 Update `allWorkspaceMembers`, `test.allWorkspaceMembers`, `clippy.allWorkspaceMembers` to use filtered set

## 5. Auto mode forwarding

- [x] 5.1 Add `members ? null` parameter to `buildFromUnitGraphAuto` in `auto.nix`
- [x] 5.2 Forward `members` to inner `buildFromUnitGraph` call
- [x] 5.3 When `members` is set, pass `--members` to the unit2nix invocation inside the IFD derivation

## 6. Tests

- [x] 6.1 Unit test: `merge()` with `members_filter` selecting one of two members — only selected in workspace_members and roots, all crates retained
- [x] 6.2 Unit test: `merge()` with invalid member name — returns error with valid members list
- [x] 6.3 CLI validation: `--members` + `--package` conflict checked in `run.rs`
- [x] 6.4 Nix test: `members = ["sample-bin"]` on sample workspace — `sample-members-filter` check passes (15/15)
- [x] 6.5 Nix validation: `builtins.throw` on invalid member names implemented

## 7. Documentation

- [x] 7.1 Update CLI `--help` (clap doc comment handles this)
- [x] 7.2 Update README CLI section with `--members` flag
- [x] 7.3 Update README Nix API section with `members` parameter
- [x] 7.4 Update template `flake.nix` with commented-out `members` example
- [x] 7.5 Update napkin with session notes
