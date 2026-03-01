## Context

unit2nix has a working CLI and Nix consumer but lacks onboarding ergonomics. Users must manually assemble a flake.nix, and there's no guard against stale `build-plan.json` files. The project has only been validated against one real workspace.

The existing codebase:
- `src/main.rs` (860 lines): merges unit-graph + metadata + Cargo.lock into JSON
- `lib/build-from-unit-graph.nix` (230 lines): wires JSON to `buildRustCrate`
- `lib/fetch-source.nix`: resolves crate sources to Nix store paths
- `flake.nix`: packages, checks, lib output, devshell (no templates yet)

## Goals / Non-Goals

**Goals:**
- One-command project setup via `nix flake init -t github:brittonr/unit2nix`
- Fail-fast when `build-plan.json` doesn't match `Cargo.lock`
- Validate against a second real-world project to find and fix edge cases

**Non-Goals:**
- IFD support (explicit non-goal per prior discussion)
- Auto-regeneration of `build-plan.json` (that's a future CI integration)
- Bundled `-sys` crate overrides (users provide their own)

## Decisions

### 1. Staleness hash: SHA256 of Cargo.lock content

**Decision:** Hash the raw `Cargo.lock` file content with SHA256 and embed it as `cargoLockHash` in the top-level JSON output.

**Alternatives considered:**
- Hash of just the `[[package]]` entries: fragile, ordering-sensitive
- Hash of the resolved crate set: circular (the output IS the resolution)
- Cargo.lock mtime: non-deterministic, breaks in Nix store

**Rationale:** SHA256 of the full file is simple, deterministic, and catches any change to dependencies. The Nix side computes `builtins.hashFile "sha256"` on the workspace's `Cargo.lock` and compares.

### 2. Nix-side validation: eval-time assert

**Decision:** Add an assertion in `build-from-unit-graph.nix` that compares the JSON's `cargoLockHash` with `builtins.hashFile "sha256" (src + "/Cargo.lock")`. Fail with a clear error message including the regeneration command.

**Alternatives considered:**
- Warning instead of error: too easy to miss, defeats the purpose
- Check only in a separate `nix flake check`: users would skip it
- Optional check with a `skipStalenessCheck` escape hatch: yes, include this for cases where src filtering strips Cargo.lock

**Rationale:** Eval-time failure is impossible to ignore. The escape hatch handles edge cases.

### 3. Template structure: minimal flake.nix + .gitignore

**Decision:** Ship a `templates/default/` containing:
- `flake.nix` — pre-wired with unit2nix input and `buildFromUnitGraph` call
- `.gitignore` — ignores `result` and `target/`

The template references `build-plan.json` and includes comments explaining the generation step. It does NOT include a `Cargo.toml` or Rust source — users already have those.

**Alternatives considered:**
- Full scaffold with Cargo.toml + src/main.rs: overly opinionated, most users have existing projects
- Multiple templates (lib, bin, workspace): premature — start with one

### 4. Real-world validation target: ripgrep

**Decision:** Use ripgrep as the validation target. It's a well-known single-workspace project with ~90 crates, a proc-macro, build scripts, and `-sys` crates (pcre2-sys). It's stable and won't break under us.

**Alternatives considered:**
- tokio: huge (300+ crates), many features, slow to test
- nushell: very large workspace, many exotic deps
- bat: good size but fewer edge cases than ripgrep

**Rationale:** ripgrep is complex enough to stress-test the tool but small enough to iterate quickly. If issues are found, we fix them as part of this change.

## Risks / Trade-offs

- [Staleness check breaks filtered sources] → Provide `skipStalenessCheck = true` parameter
- [ripgrep needs -sys overrides we can't predict] → Document which overrides were needed; this IS the value of the exercise
- [`--unit-graph` output changes on nightly update] → Pin nightly version in CI; this is a pre-existing risk
- [Template becomes stale as API evolves] → Template is minimal (20 lines); add a CI check that `nix flake init` + `nix eval` succeeds
