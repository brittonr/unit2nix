## Context

unit2nix produces a build plan JSON that Nix consumes via `build-from-unit-graph.nix`. When the plan includes -sys crates (crates with a `links` field), users must manually write `defaultCrateOverrides` attrset entries to provide native libraries. nixpkgs ships ~60 built-in overrides in `pkgs.defaultCrateOverrides`, but common crates like `libsqlite3-sys`, `ring`, and `tikv-jemalloc-sys` are missing. Users hit opaque build failures and must reverse-engineer what each crate needs.

The current override hierarchy is flat: `pkgs.defaultCrateOverrides // userOverrides`. Users must repeat the `//` merge and know to start from `pkgs.defaultCrateOverrides`.

## Goals / Non-Goals

**Goals:**
- Most projects with common -sys crates build without any user-written overrides
- When overrides are missing, the user gets an actionable message (crate name + suggested override)
- Users can still override anything — the built-in registry is additive, not restrictive
- Test builds (bat, nushell, fd) simplified — most per-test overrides become unnecessary

**Non-Goals:**
- Auto-detecting arbitrary C libraries at eval time (too fragile, belongs in nixpkgs)
- Replacing nixpkgs' `defaultCrateOverrides` (we layer on top)
- Handling -sys crates that vendor+build from source without any override (e.g., `tikv-jemalloc-sys` needs `make` — that's project-specific)
- CLI subcommand for generating override Nix code (keep it simple: trace warnings + docs)

## Decisions

### 1. Ship a curated `lib/crate-overrides.nix` alongside nixpkgs defaults

**Choice:** A new Nix file exporting a function `{ pkgs }: attrset` with overrides for ~15 common crates not in nixpkgs' `defaultCrateOverrides`.

**Rationale:** nixpkgs' list is maintained by the Nix community and moves slowly. unit2nix can ship a more aggressive set tuned for `buildRustCrate` specifically. Separating it from nixpkgs means we can iterate without waiting for upstream PRs.

**Alternatives considered:**
- Contribute to nixpkgs `defaultCrateOverrides` — too slow, and nixpkgs targets the general case; we target unit2nix's specific buildRustCrate usage.
- Generate overrides from `links` field heuristics — too fragile. `links = "z"` doesn't reliably map to `pkgs.zlib`.

### 2. Three-layer merge: nixpkgs → unit2nix built-ins → user overrides

**Choice:** Default merge order is `pkgs.defaultCrateOverrides // unit2nixOverrides // extraCrateOverrides`. User-provided `extraCrateOverrides` always wins.

**Rationale:** Users should only write overrides for project-specific crates. The existing `defaultCrateOverrides` param continues to work for full control (bypasses the built-in layer).

**API change:**
```nix
# Before (user must repeat boilerplate for known crates):
buildFromUnitGraph {
  defaultCrateOverrides = pkgs.defaultCrateOverrides // {
    libgit2-sys = attrs: { ... };
    libz-sys = attrs: { ... };
    my-custom-sys = attrs: { ... };
  };
};

# After (built-ins handle libgit2-sys and libz-sys):
buildFromUnitGraph {
  extraCrateOverrides = {
    my-custom-sys = attrs: { ... };
  };
};
```

### 3. Eval-time `builtins.trace` warnings for unmatched `links` fields

**Choice:** During crate set construction, if a crate has a `links` field and its `crateName` is not in the merged override set, emit a trace warning with the crate name and a pointer to the docs.

**Rationale:** This is cheap (no extra eval pass), non-breaking (trace doesn't fail builds), and actionable. The warning fires even if the build succeeds (some `links` crates don't actually need overrides, e.g., `rayon-core`), but false positives are harmless — the trace tells users to check.

**Filtering:** Some `links` values are Rust-internal (e.g., `rayon-core`, `prettyplease02`) and never need native overrides. The registry includes a `knownNoOverride` set to suppress warnings for these.

### 4. `--check-overrides` CLI flag on the Rust side

**Choice:** A flag that reads the build plan JSON, lists all crates with `links` fields, cross-references against a known-crate list compiled into the binary, and prints a report.

**Rationale:** Users can check before `nix build` — faster feedback loop than waiting for a build failure. The known-crate list is a simple `HashMap<&str, &str>` mapping crate names to human-readable notes (e.g., "needs pkg-config + zlib").

### 5. Keep `defaultCrateOverrides` param for backward compatibility

**Choice:** The existing parameter still works and, when provided, replaces the entire base layer (nixpkgs + unit2nix built-ins). The new `extraCrateOverrides` param is additive.

**Rationale:** No breaking changes. Users who have working `defaultCrateOverrides` overrides don't need to change anything. New users use `extraCrateOverrides` for less boilerplate.

## Risks / Trade-offs

- **[Stale built-in overrides]** → Pin overrides to well-known patterns (pkg-config + lib). Review when bumping nixpkgs input. The override registry is small (~15 entries) and easy to audit.
- **[False positive warnings]** → The `knownNoOverride` suppression set handles Rust-internal `links`. For others, trace warnings are advisory — they don't break builds.
- **[Override function conflicts]** → If nixpkgs adds an override for a crate we also ship, the user gets both (nixpkgs' is overridden by ours). This is fine since our overrides are designed for `buildRustCrate` specifically.
- **[Increased API surface]** → `extraCrateOverrides` is one new param. The complexity cost is low and the ergonomic benefit is high.
