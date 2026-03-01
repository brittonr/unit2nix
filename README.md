# unit2nix

Per-crate Nix build plans from Cargo's unit graph. Replaces crane's monolithic workspace builds with individual `buildRustCrate` derivations — one per crate — so changing one crate doesn't rebuild everything.

## Why

With crane (or `buildRustPackage`), any `Cargo.lock` change rebuilds all crates. With unit2nix, Nix caches each crate independently:

| Change | unit2nix rebuilds | Crane rebuilds |
|--------|------------------|----------------|
| Edit a local crate | 31 | 457 |
| Bump `serde` | 133 | 457 |
| Bump `tokio` | 70 | 457 |
| Bump a leaf crate | 41 | 457 |

*Measured on a 457-crate Rust workspace.*

## How it works

```
cargo build --unit-graph ─┐
                          │
cargo metadata ───────────┼─→ unit2nix (Rust) ─→ build-plan.json
                          │
Cargo.lock checksums ─────┤
                          │
nix-prefetch-git ─────────┘   (git deps only)

build-plan.json ─→ build-from-unit-graph.nix ─→ 457 buildRustCrate derivations
```

Cargo does all dependency resolution, feature expansion, and platform filtering. unit2nix merges three Cargo outputs into one JSON. Git dependencies are prefetched at generation time so the Nix consumer can use fixed-output derivations — no `--impure` needed. The Nix consumer is a thin wrapper — no `cfg()` evaluator, no feature resolver.

## Quickstart

### Generate a build plan

```bash
# Requires nightly Rust (for --unit-graph)
cargo +nightly run --manifest-path /path/to/unit2nix/Cargo.toml -- \
  --manifest-path ./Cargo.toml \
  --features my-feature \
  -o build-plan.json
```

### Use in a flake

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    unit2nix.url = "github:brittonr/unit2nix";
  };

  outputs = { nixpkgs, unit2nix, ... }:
    let
      pkgs = nixpkgs.legacyPackages.x86_64-linux;
      ws = unit2nix.lib.x86_64-linux.buildFromUnitGraph {
        inherit pkgs;
        src = ./.;
        resolvedJson = ./build-plan.json;
      };
    in {
      # Single binary
      packages.x86_64-linux.default = ws.workspaceMembers."my-crate".build;

      # All workspace members
      packages.x86_64-linux.all = ws.allWorkspaceMembers;
    };
}
```

### Or import directly (no flake input needed)

```nix
let
  unit2nix-src = builtins.fetchGit {
    url = "https://github.com/brittonr/unit2nix";
    rev = "...";
  };
  ws = import "${unit2nix-src}/lib/build-from-unit-graph.nix" {
    inherit pkgs;
    src = ./.;
    resolvedJson = ./build-plan.json;
  };
in ws.workspaceMembers."my-crate".build
```

## CLI

```
unit2nix [OPTIONS]

Options:
  --manifest-path <PATH>    Path to Cargo.toml [default: ./Cargo.toml]
  --features <FEATURES>     Comma-separated features to enable
  --bin <NAME>              Build a specific binary target
  -p, --package <NAME>      Build a specific package
  --all-features            Enable all features
  --no-default-features     Disable default features
  --target <TRIPLE>         Target triple (e.g. aarch64-unknown-linux-gnu)
  -o, --output <FILE>       Output file [default: stdout]
```

## Nix API

### `buildFromUnitGraph`

```nix
buildFromUnitGraph {
  pkgs;                                              # nixpkgs instance
  src;                                               # workspace source root
  resolvedJson;                                      # path to build-plan.json
  buildRustCrateForPkgs ? pkgs: pkgs.buildRustCrate; # override buildRustCrate
  defaultCrateOverrides ? pkgs.defaultCrateOverrides; # crate-specific overrides
}
```

Returns:

| Attribute | Type | Description |
|-----------|------|-------------|
| `workspaceMembers` | `{ name = { packageId, build }; }` | Workspace members by crate name |
| `rootCrate` | `{ packageId, build }` or `null` | Root package (if any) |
| `allWorkspaceMembers` | derivation | `symlinkJoin` of all members |
| `resolved` | attrset | Raw parsed JSON |
| `builtCrates` | `{ crates = { pkgId = drv; }; }` | All crate derivations by package ID |

### Crate overrides

For crates with native dependencies (`-sys` crates):

```nix
buildFromUnitGraph {
  inherit pkgs src;
  resolvedJson = ./build-plan.json;
  defaultCrateOverrides = pkgs.defaultCrateOverrides // {
    openssl-sys = attrs: {
      nativeBuildInputs = [ pkgs.pkg-config ];
      buildInputs = [ pkgs.openssl.dev ];
    };
    prost-build = attrs: {
      nativeBuildInputs = [ pkgs.protobuf ];
    };
    my-crate = attrs: {
      SOME_ENV_VAR = "value";
    };
  };
}
```

### Cross-compilation

Generate a build plan for the target:

```bash
unit2nix --target aarch64-unknown-linux-gnu -o build-plan-aarch64.json
```

Use with cross-compilation pkgs:

```nix
let
  crossPkgs = import nixpkgs { system = "x86_64-linux"; crossSystem = "aarch64-linux"; };
in buildFromUnitGraph {
  pkgs = crossPkgs;
  src = ./.;
  resolvedJson = ./build-plan-aarch64.json;
}
```

## vs crate2nix

| | unit2nix | crate2nix |
|---|---------|-----------|
| **Resolver** | Cargo (unit graph) | Reimplemented in Rust |
| **Platform filtering** | Done by Cargo | 144-line `cfg()` evaluator in Nix |
| **SHA256 hashes** | Cargo.lock + `nix-prefetch-git` for git deps | `nix-prefetch-url` per crate |
| **Rust code** | 860 lines | 4,661 lines |
| **Nix code** | 230 lines | 274 lines |
| **Stability** | Nightly (`--unit-graph`) | Stable (`cargo metadata`) |
| **Cross-compilation** | One JSON per target | One JSON, filtered at eval time |

unit2nix trades Cargo API stability (nightly requirement) for correctness (Cargo's own resolver) and simplicity (6x less code).

## Testing

```bash
cargo test              # 12 unit tests
nix flake check         # 4 checks: sample build + 3 NixOS VM tests
```

## Requirements

- **Nightly Rust** — `cargo build --unit-graph` requires `-Z unstable-options`
- **Nix** with flakes enabled
- **nix-prefetch-git** — for prefetching git dependency hashes (bundled when installed via flake)

## Repository structure

```
src/main.rs                     # Rust merger (unit-graph + metadata + Cargo.lock)
lib/build-from-unit-graph.nix   # Nix consumer (buildRustCrate wiring)
lib/fetch-source.nix            # Source fetching (local, crates.io, git+subdir)
flake.nix                       # Flake with lib, packages, checks, devshell
tests/vm/                       # NixOS VM integration tests
sample_workspace/               # 4-crate test workspace (lib, bin, proc-macro, build-script)
```

## License

MIT
