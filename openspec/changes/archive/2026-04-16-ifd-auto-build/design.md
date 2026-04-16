## Context

crate2nix's `tools.nix` demonstrates a working IFD vendoring pattern (~200 lines of Nix):

1. Parse `Cargo.lock` at eval time via `lib.importTOML`
2. Fetch each crate source as a fixed-output derivation (checksums from `Cargo.lock` — no user input)
3. Link sources into a vendor directory via `pkgs.linkFarm`
4. Write a cargo config pointing `[source.crates-io]` at the vendor dir
5. Run the generator inside `mkDerivation` with vendored `CARGO_HOME`
6. Import the result (IFD)

This is pure — all fetches are FODs with known hashes. IFD is orthogonal to purity; it's blocked only by `allow-import-from-derivation = false` (Hydra default).

We adapt this pattern for unit2nix: vendor → run `unit2nix` → IFD the JSON → feed to existing `buildFromUnitGraph`.

## Goals / Non-Goals

**Goals:**
- `buildFromUnitGraphAuto { pkgs; src; }` — two required args, no `resolvedJson`, no hashes
- Works with `nix flake check` and `--pure-eval`
- crates.io deps need zero user input (checksums from Cargo.lock)
- Git deps work if `crate-hashes.json` is present (same as crate2nix)
- Compose cleanly with existing `buildFromUnitGraph` — auto generates the JSON that manual mode checks in

**Non-Goals:**
- Hydra support (IFD disabled there — users on Hydra use manual mode)
- Alternative registries (same limitation as existing `fetch-source.nix`)
- Replacing manual mode — both coexist, user picks

## Decisions

### 1. Separate `lib/vendor.nix` module

**Choice**: Vendoring logic in its own file, independent of auto-build.

**Why**: The vendor module is useful on its own (e.g., users who want vendored deps for other purposes). Clean separation: `vendor.nix` produces a cargo config + vendor dir, `auto.nix` uses it to run unit2nix.

### 2. Parse Cargo.lock with `lib.importTOML`

**Choice**: Use Nix's built-in TOML parser at eval time.

**Why**: `Cargo.lock` is TOML. `lib.importTOML` is pure, fast, and avoids a derivation. Gives us package names, versions, sources, and checksums — everything needed to fetch.

**Alternative considered**: Embed lock info in the JSON output. Rejected — would couple the auto mode to a specific unit2nix version, and the info is already in `Cargo.lock`.

### 3. Reuse `buildFromUnitGraph` internally

**Choice**: `buildFromUnitGraphAuto` generates a JSON file via IFD and passes it to `buildFromUnitGraph` with `skipStalenessCheck = true`.

**Why**: Zero duplication. The auto path generates exactly what the manual path checks in. All downstream Nix logic (crate building, source fetching, overrides) is shared.

### 4. Git deps require `crate-hashes.json`

**Choice**: For git deps, look up hashes in an optional `crate-hashes.json` at the workspace root (same convention as crate2nix). Without it, fall back to `builtins.fetchGit` (works but requires `--impure` in some Nix configs).

**Why**: `Cargo.lock` has checksums for crates.io packages but not for git deps. The `crate-hashes.json` file fills this gap. crate2nix already established this convention, so users migrating from crate2nix can reuse their existing file.

### 5. Forward `defaultCrateOverrides` and `buildRustCrateForPkgs`

**Choice**: `buildFromUnitGraphAuto` accepts the same optional args as `buildFromUnitGraph` and forwards them.

**Why**: The auto mode should be a drop-in. Users with `-sys` crate overrides shouldn't need to change anything except removing `resolvedJson`.

## Risks / Trade-offs

- **Eval-time builds**: IFD blocks evaluation until the vendoring + unit2nix derivation finishes. For large workspaces, this adds seconds to first eval. Subsequent evals are cached. → Acceptable for the convenience gained.
- **Cargo.lock required at eval time**: The `src` must contain a `Cargo.lock` accessible to Nix's evaluator (i.e., not filtered out). → Document this; already true for the staleness check.
- **cargo version coupling**: The vendored cargo config must match what unit2nix expects. Since we bundle cargo in the wrapper, this is controlled. → Low risk.
- **Git dep UX**: Users with git deps need `crate-hashes.json` or `--impure`. → Same as crate2nix, well-understood trade-off.
