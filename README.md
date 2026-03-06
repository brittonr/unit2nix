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

## Install

**Via Nix (recommended)** — no install needed, use `nix run`:

```bash
nix run github:brittonr/unit2nix -- --help
```

**Via cargo** — installs `unit2nix` and `cargo unit2nix` subcommand:

```bash
cargo install cargo-unit2nix
```

## Quickstart

### 1. Scaffold a flake

```bash
nix flake init -t github:brittonr/unit2nix
```

This creates a `flake.nix` pre-wired with unit2nix. Edit it to set your crate name.

### 2. Generate a build plan

```bash
unit2nix
```

This writes `build-plan.json` (the default). Or via the cargo subcommand:

```bash
cargo unit2nix
```

### 3. Build

```bash
nix build
```

### Or use auto mode (no manual step)

Auto mode uses IFD to generate the build plan at eval time — no `build-plan.json` to maintain:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    unit2nix.url = "github:brittonr/unit2nix";
  };

  outputs = { nixpkgs, unit2nix, ... }:
    let
      pkgs = nixpkgs.legacyPackages.x86_64-linux;
      ws = unit2nix.lib.x86_64-linux.buildFromUnitGraphAuto {
        inherit pkgs;
        src = ./.;
      };
    in {
      packages.x86_64-linux.default = ws.workspaceMembers."my-crate".build;
    };
}
```

> **Auto vs manual mode**: Auto mode requires [IFD](https://nix.dev/manual/nix/latest/language/import-from-derivation) (enabled by default in Nix, disabled on Hydra). If you use Hydra or need maximum eval performance, use manual mode with a checked-in `build-plan.json`. Both produce identical builds.

### Or set up manually

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

### Or use the nixpkgs overlay

The overlay puts unit2nix on `pkgs.unit2nix` — no `system` threading needed:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    unit2nix.url = "github:brittonr/unit2nix";
  };

  outputs = { nixpkgs, unit2nix, ... }:
    let
      pkgs = import nixpkgs {
        system = "x86_64-linux";
        overlays = [ unit2nix.overlays.default ];
      };
      ws = pkgs.unit2nix.buildFromUnitGraph {
        src = ./.;
        resolvedJson = ./build-plan.json;
      };
    in {
      packages.x86_64-linux.default = ws.workspaceMembers."my-crate".build;
    };
}
```

The overlay provides `pkgs.unit2nix.{cli, buildFromUnitGraph, buildFromUnitGraphAuto, crateOverrides, isKnownNoOverride}`. Since `pkgs` is implicit, you don't need to pass it — though you can override it for cross-compilation.

### Or use the flake-parts module (least boilerplate)

The [flake-parts](https://flake.parts) module auto-wires `packages`, `checks`, `devShells`, and `apps` from a few lines of config:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    unit2nix.url = "github:brittonr/unit2nix";
  };

  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ inputs.unit2nix.flakeModules.default ];
      systems = [ "x86_64-linux" "aarch64-linux" ];

      unit2nix = {
        enable = true;
        src = ./.;
        resolvedJson = ./build-plan.json;  # null for auto mode
        defaultPackage = "my-bin";         # or null for allWorkspaceMembers
      };
    };
}
```

This gives you:
- `packages.default` — your main binary (or all workspace members)
- `packages.<name>` — one per workspace member
- `checks.unit2nix-clippy` — clippy on all workspace members
- `checks.unit2nix-tests` — runs `#[test]` functions
- `devShells.default` — shell with `unit2nix`, `cargo`, `rustc`, `rust-analyzer`
- `apps.update-plan` — regenerate `build-plan.json` (manual mode only)

See all options: `enable`, `src`, `resolvedJson`, `workspaceDir`, `defaultPackage`, `members`, `extraCrateOverrides`, `checks.{clippy, tests, overrides}`, `devShell.{enable, extraPackages}`, `rustToolchain`.

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
  -o, --output <FILE>       Output file [default: build-plan.json]
  --stdout                 Write to stdout instead of a file
  --members <NAMES>         Build only specific workspace members (comma-separated)
  --include-dev             Include dev-dependencies for test support
  --check-overrides         Check -sys crate override coverage (reads existing plan)
  --json                    Machine-readable JSON output (with --check-overrides)
  --no-check                Skip automatic override check after generation
```

## Nix API

### `buildFromUnitGraph` (manual mode)

```nix
buildFromUnitGraph {
  pkgs;                                              # nixpkgs instance
  src;                                               # workspace source root
  resolvedJson;                                      # path to build-plan.json
  buildRustCrateForPkgs ? pkgs: pkgs.buildRustCrate; # override buildRustCrate
  extraCrateOverrides ? {};                           # project-specific -sys overrides
  skipStalenessCheck ? false;                         # skip Cargo.lock hash check
  members ? null;                                     # filter workspace members (list of names)
}
```

### `buildFromUnitGraphAuto` (auto mode)

```nix
buildFromUnitGraphAuto {
  pkgs;                                              # nixpkgs instance
  src;                                               # workspace source root (must contain Cargo.lock)
  buildRustCrateForPkgs ? pkgs: pkgs.buildRustCrate; # override buildRustCrate
  extraCrateOverrides ? {};                           # project-specific -sys overrides
}
```

No `resolvedJson` needed — the build plan is generated at eval time via IFD. Crate sources are vendored from `Cargo.lock` checksums (no network in the build sandbox). If the workspace has git dependencies, place a `crate-hashes.json` at the workspace root with SHA256 hashes:

```json
{
  "https://github.com/user/repo?branch=main#crate-name@1.0.0": "sha256-..."
}
```

This follows the same convention as crate2nix, so existing `crate-hashes.json` files work.

### Keeping build-plan.json up to date

Regenerate whenever `Cargo.toml` or `Cargo.lock` changes:

```bash
unit2nix                       # writes build-plan.json
cargo unit2nix                 # via cargo subcommand
```

After generating, unit2nix automatically prints an override coverage summary showing which `-sys` crates are covered, which need attention, and exact override snippets. Use `--no-check` to suppress this in scripts.

unit2nix embeds a SHA256 hash of `Cargo.lock` in `build-plan.json`. At Nix eval time, the hash is compared against the current `Cargo.lock`. If they differ, evaluation fails with a clear error telling you exactly what to run.

To disable (e.g., when source filtering strips `Cargo.lock`):

```nix
buildFromUnitGraph {
  # ...
  skipStalenessCheck = true;
}
```

The check is also skipped automatically for build plans generated by older versions of unit2nix (backwards compatible).

Returns:

| Attribute | Type | Description |
|-----------|------|-------------|
| `workspaceMembers` | `{ name = { packageId, build }; }` | Workspace members by crate name |
| `rootCrate` | `{ packageId, build }` or `null` | Root package (if any) |
| `allWorkspaceMembers` | derivation | `symlinkJoin` of all members |
| `test.check` | `{ name = drv; }` | Run `#[test]` for each workspace member (requires `--include-dev`) |
| `test.workspaceMembers` | `{ name = { packageId, build }; }` | Workspace members built with dev deps |
| `clippy.allWorkspaceMembers` | derivation | Lint all workspace members with clippy |
| `resolved` | attrset | Raw parsed JSON |
| `builtCrates` | `{ crates = { pkgId = drv; }; }` | All crate derivations by package ID |

### Crate overrides

Common `-sys` crates work out of the box — unit2nix ships built-in overrides for `ring`, `tikv-jemalloc-sys`, `onig_sys`, and others, and inherits nixpkgs' overrides for `openssl-sys`, `libgit2-sys`, `libz-sys`, etc. Check coverage with:

```bash
unit2nix --check-overrides -o build-plan.json        # human-readable
unit2nix --check-overrides --json -o build-plan.json  # machine-readable (for CI)
```

For CI, add an override coverage check to your flake:

```nix
checks.overrides = pkgs.runCommand "check-overrides" {
  nativeBuildInputs = [ unit2nix pkgs.jq ];
} ''
  unit2nix --check-overrides --json -o ${./build-plan.json} > report.json
  missing=$(jq -r '.missing' report.json)
  if [ "$missing" -gt 0 ]; then
    echo "Missing overrides detected"
    exit 1
  fi
  cp report.json $out
'';
```

For project-specific crates, use `extraCrateOverrides`:

```nix
buildFromUnitGraph {
  inherit pkgs src;
  resolvedJson = ./build-plan.json;
  extraCrateOverrides = {
    my-custom-sys = attrs: {
      nativeBuildInputs = [ pkgs.pkg-config ];
      buildInputs = [ pkgs.some-library ];
    };
  };
}
```

### Running tests in Nix

Generate a build plan with dev dependencies, then use `test.check` to compile and run `#[test]` functions inside the Nix sandbox:

```bash
# Generate with dev deps
cargo unit2nix --include-dev -o build-plan.json

# Run tests for a specific workspace member
nix build .#test.check.my-crate

# Or add to flake checks for CI
checks.x86_64-linux.my-crate-tests = ws.test.check."my-crate";
```

Each `test.check.<name>` derivation compiles the workspace member with `buildTests = true` (producing test binaries via `rustc --test`), then executes them. Dependencies are built normally and cached — only the workspace member itself is recompiled for testing.

### Cross-compilation

Cross-compilation requires two things: a build plan resolved for the target, and a nixpkgs instance configured for cross-compilation.

**1. Generate a target-specific build plan:**

```bash
unit2nix --target aarch64-unknown-linux-gnu -o build-plan-aarch64.json
```

Cargo filters the unit graph to only include crates needed for that target — platform-specific dependencies (e.g., `winapi` on Linux) are excluded. The `--target` triple is stored in the JSON so the Nix consumer can validate it.

**2. Use with cross-compilation pkgs:**

```nix
let
  crossPkgs = pkgs.pkgsCross.aarch64-multiplatform;
in buildFromUnitGraph {
  pkgs = crossPkgs;
  src = ./.;
  resolvedJson = ./build-plan-aarch64.json;
}
```

Or equivalently:

```nix
let
  crossPkgs = import nixpkgs { system = "x86_64-linux"; crossSystem = "aarch64-linux"; };
in buildFromUnitGraph {
  pkgs = crossPkgs;
  src = ./.;
  resolvedJson = ./build-plan-aarch64.json;
}
```

**Target mismatch warning:** If the build plan's `--target` doesn't match `pkgs.stdenv.hostPlatform`, unit2nix emits a trace warning during evaluation. This catches silent mismatches (e.g., using an x86_64 plan with aarch64 pkgs).

**Each target needs its own build plan** because Cargo resolves different dependency trees per target. This is by design — it means the Nix consumer never needs a `cfg()` evaluator.

Cross-compilation is validated in CI: the sample workspace (including proc-macro and build script) is cross-compiled to aarch64 on every `nix flake check`.

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

## Tested projects

| Project | Crates | Workspace members | Notes |
|---------|--------|-------------------|-------|
| sample_workspace | 15 | 4 | lib, bin, proc-macro, build-script |
| [ripgrep](https://github.com/BurntSushi/ripgrep) | 34 | 9 | Pure Rust, zero overrides needed |
| [fd](https://github.com/sharkdp/fd) | 59 | 1 | Zero overrides — jemalloc-sys covered by built-ins |
| [bat](https://github.com/sharkdp/bat) | 168 | 1 | Near-zero overrides — libgit2-sys, libz-sys, onig_sys all auto-covered |
| [nushell](https://github.com/nushell/nushell) | 519 | 29 | Zero overrides — sqlite, ring auto-covered |
| Private workspace | 457 | — | Full production build |

## Testing

```bash
cargo test              # 48 unit tests
nix flake check         # 18 checks: sample build/clippy/test + overlay/module smoke tests + override coverage + fd/bat/ripgrep/nushell validation + cross-compilation + 3 NixOS VM tests
```

## Requirements

- **Nightly Rust** — `cargo build --unit-graph` requires `-Z unstable-options`
- **Nix** with flakes enabled
- **nix-prefetch-git** — for prefetching git dependency hashes (bundled when installed via flake)

## Repository structure

```
src/
  main.rs                       # Entry point
  cargo_main.rs                 # cargo-unit2nix subcommand entry point
  cli.rs                        # CLI argument parsing (clap)
  cargo.rs                      # Cargo command runners (unit-graph, metadata, Cargo.lock)
  unit_graph.rs                 # Unit graph deserialization types
  metadata.rs                   # Cargo metadata + Cargo.lock types
  output.rs                     # Output serialization types (NixBuildPlan, NixCrate, etc.)
  source.rs                     # Source parsing (crates.io, git, local, registry)
  merge.rs                      # Core merge logic (unit-graph + metadata + Cargo.lock → plan)
  prefetch.rs                   # Git dependency SHA256 prefetching
lib/build-from-unit-graph.nix   # Nix consumer (buildRustCrate wiring)
lib/fetch-source.nix            # Source fetching (local, crates.io, git+subdir)
nix/overlay.nix                 # Nixpkgs overlay (pkgs.unit2nix.*)
flake-modules/default.nix       # Flake-parts module (auto-wires packages/checks/devShell)
flake.nix                       # Flake with lib, packages, checks, overlays, flakeModules
tests/vm/                       # NixOS VM integration tests
sample_workspace/               # 4-crate test workspace (lib, bin, proc-macro, build-script)
```

## License

MIT
