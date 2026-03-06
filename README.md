# unit2nix

Per-crate Nix builds from Cargo's unit graph.

Crane and `buildRustPackage` treat your entire dependency tree as one big derivation. Change one line in `Cargo.lock` and everything rebuilds. unit2nix generates a separate `buildRustCrate` derivation for each crate, so Nix can cache them independently.

On a 457-crate workspace:

| What changed | unit2nix rebuilds | Crane rebuilds |
|---|---|---|
| Edit a local crate | 31 | 457 |
| Bump `serde` | 133 | 457 |
| Bump `tokio` | 70 | 457 |
| Bump a leaf dep | 41 | 457 |

## How it works

Cargo already knows how to resolve dependencies, expand features, and filter by platform. unit2nix just asks it:

```
cargo build --unit-graph ─┐
cargo metadata ───────────┼─→ unit2nix ─→ build-plan.json
Cargo.lock checksums ─────┤
nix-prefetch-git ─────────┘   (git deps only)
```

The JSON output feeds a thin Nix file (~500 lines) that wires up `buildRustCrate` calls. No `cfg()` evaluator, no feature resolver, no reimplemented dependency logic.

Git deps are prefetched at generation time so builds stay pure — no `--impure` needed.

## Getting started

```bash
# scaffold a flake in your Rust project
nix flake init -t github:brittonr/unit2nix

# generate the build plan
nix run github:brittonr/unit2nix

# build
nix build
```

Regenerate whenever `Cargo.toml` or `Cargo.lock` changes. unit2nix embeds a `Cargo.lock` hash in the plan and fails at eval time if they drift, so you'll know.

## Usage modes

### Auto mode (IFD, no checked-in JSON)

The build plan is generated at Nix eval time. Nothing to maintain, but requires [import-from-derivation](https://nix.dev/manual/nix/latest/language/import-from-derivation) (enabled by default, disabled on Hydra).

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

### Manual mode (checked-in `build-plan.json`)

Better for Hydra, CI caching, and fast eval.

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
      packages.x86_64-linux.default = ws.workspaceMembers."my-crate".build;
      packages.x86_64-linux.all = ws.allWorkspaceMembers;
    };
}
```

### flake-parts module (least boilerplate)

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
        defaultPackage = "my-bin";
      };
    };
}
```

Gives you `packages.default`, per-member packages, clippy checks, test checks, a dev shell, and an `apps.update-plan` command — all wired up automatically.

### Overlay

```nix
let
  pkgs = import nixpkgs {
    system = "x86_64-linux";
    overlays = [ unit2nix.overlays.default ];
  };
  ws = pkgs.unit2nix.buildFromUnitGraph {
    src = ./.;
    resolvedJson = ./build-plan.json;
  };
in ws.workspaceMembers."my-crate".build
```

## CLI reference

```
unit2nix [OPTIONS]

  --manifest-path <PATH>    Path to Cargo.toml [default: ./Cargo.toml]
  --features <FEATURES>     Comma-separated features to enable
  --all-features            Enable all features
  --no-default-features     Disable default features
  --bin <NAME>              Build a specific binary target
  -p, --package <NAME>      Build a specific package
  --members <NAMES>         Workspace members to include (comma-separated)
  --target <TRIPLE>         Cross-compilation target (e.g. aarch64-unknown-linux-gnu)
  --include-dev             Include dev-dependencies (needed for nix test support)
  -o, --output <FILE>       Output file [default: build-plan.json]
  --stdout                  Write to stdout
  --force                   Regenerate even if inputs haven't changed
  --check-overrides         Report -sys crate override coverage
  --json                    Machine-readable output (with --check-overrides)
  --no-check                Skip override check after generation
```

Also available as `cargo unit2nix`.

## Nix API

### Return value

Both `buildFromUnitGraph` and `buildFromUnitGraphAuto` return:

| Attribute | Description |
|---|---|
| `workspaceMembers.<name>.build` | Built workspace member |
| `rootCrate.build` | Root package (if single-package project) |
| `allWorkspaceMembers` | `symlinkJoin` of all members |
| `test.check.<name>` | Run `#[test]` for a member (requires `--include-dev`) |
| `clippy.allWorkspaceMembers` | Clippy lint all members |
| `builtCrates.crates.<pkgId>` | Every crate derivation by package ID |

### `-sys` crate overrides

Common `-sys` crates (`ring`, `openssl-sys`, `libgit2-sys`, `tikv-jemalloc-sys`, etc.) are handled automatically via built-in overrides plus nixpkgs' own `defaultCrateOverrides`.

Check your coverage:

```bash
unit2nix --check-overrides -o build-plan.json
```

For project-specific crates:

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

### Cross-compilation

Each target needs its own build plan (because Cargo resolves different dep trees per platform):

```bash
unit2nix --target aarch64-unknown-linux-gnu -o build-plan-aarch64.json
```

```nix
buildFromUnitGraph {
  pkgs = pkgs.pkgsCross.aarch64-multiplatform;
  src = ./.;
  resolvedJson = ./build-plan-aarch64.json;
}
```

unit2nix warns at eval time if the plan's target doesn't match `pkgs.stdenv.hostPlatform`.

### Testing in Nix

```bash
cargo unit2nix --include-dev
nix build .#test.check.my-crate
```

### Git dependencies in auto mode

If your workspace has git deps, place a `crate-hashes.json` at the root with SHA256 hashes (same format as crate2nix):

```json
{
  "https://github.com/user/repo?branch=main#crate-name@1.0.0": "sha256-..."
}
```

## Compared to crate2nix

| | unit2nix | crate2nix |
|---|---|---|
| Resolver | Cargo itself (unit graph) | Reimplemented in Rust |
| Platform filtering | Cargo does it | 144-line `cfg()` evaluator in Nix |
| Rust LOC | ~3,400 | ~4,700 |
| Nix LOC | ~520 | ~270 |
| Stability | Nightly (needs `--unit-graph`) | Stable Rust |
| Cross-compilation | One JSON per target | One JSON, filtered at eval |

The tradeoff: unit2nix needs nightly Cargo, but delegates all the hard parts (resolution, features, platform filtering) to Cargo instead of reimplementing them.

## Tested on

| Project | Crates | Notes |
|---|---|---|
| [ripgrep](https://github.com/BurntSushi/ripgrep) | 34 | Pure Rust, zero overrides |
| [fd](https://github.com/sharkdp/fd) | 59 | jemalloc covered by built-ins |
| [bat](https://github.com/sharkdp/bat) | 168 | libgit2, libz, onig all auto-covered |
| [nushell](https://github.com/nushell/nushell) | 519 | sqlite, ring auto-covered |
| Private workspace | 457 | Production build |

## Development

```bash
cargo test              # unit tests
nix flake check         # full suite: sample builds, clippy, overlay/module smoke tests,
                        # override coverage, fd/bat/ripgrep/nushell validation,
                        # cross-compilation, NixOS VM integration tests
```

## Requirements

- Nightly Rust (`cargo --unit-graph` is unstable)
- Nix with flakes

## License

MIT
