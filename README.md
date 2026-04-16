# unit2nix

Per-crate Nix builds from Cargo's unit graph.

On a 457-crate workspace:

| What changed | unit2nix rebuilds | Crane rebuilds |
|---|---|---|
| Edit a local crate | 31 | 457 |
| Bump `serde` | 133 | 457 |
| Bump `tokio` | 70 | 457 |
| Bump a leaf dep | 41 | 457 |

## How it works

```
cargo build --unit-graph ─┐
cargo metadata ───────────┼─→ unit2nix ─→ build-plan.json
Cargo.lock checksums ─────┤
nix-prefetch-git ─────────┘   (git deps only)
```

The Nix side reads the JSON and calls `buildRustCrate` for each crate.

## Quick start

```bash
nix flake init -t github:brittonr/unit2nix
nix run github:brittonr/unit2nix
nix build
```

The build plan embeds a `Cargo.lock` hash. If `Cargo.toml` or `Cargo.lock` changes, re-run `unit2nix` — Nix eval fails on hash mismatch.

## Flake integration

### Manual mode (checked-in JSON, no IFD)

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
    };
}
```

### Auto mode (IFD, nothing checked in)

```nix
ws = unit2nix.lib.x86_64-linux.buildFromUnitGraphAuto {
  inherit pkgs;
  src = ./.;
};
```

Plan is generated at eval time via [import-from-derivation](https://nix.dev/manual/nix/latest/language/import-from-derivation). Works everywhere except Hydra.

### flake-parts

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

### Overlay

```nix
pkgs = import nixpkgs {
  system = "x86_64-linux";
  overlays = [ unit2nix.overlays.default ];
};
ws = pkgs.unit2nix.buildFromUnitGraph {
  src = ./.;
  resolvedJson = ./build-plan.json;
};
```

## Return value

Both `buildFromUnitGraph` and `buildFromUnitGraphAuto` return:

| Attribute | Description |
|---|---|
| `workspaceMembers.<name>.build` | Built workspace member |
| `rootCrate.build` | Root package (single-package projects) |
| `allWorkspaceMembers` | `symlinkJoin` of all members |
| `test.workspaceMembers.<name>.build` | Workspace member built from an isolated per-member test graph |
| `test.allWorkspaceMembers` | `symlinkJoin` of per-member test builds |
| `test.check.<name>` | `#[test]` runner built from the same per-member test graph (requires `--include-dev` or `--workspace`) |
| `clippy.allWorkspaceMembers` | Clippy all members |
| `builtCrates.crates.<pkgId>` | Every crate derivation by package ID |

## `-sys` crate overrides

`ring`, `openssl-sys`, `libgit2-sys`, `tikv-jemalloc-sys`, and other common `-sys` crates are handled by built-in overrides plus nixpkgs' `defaultCrateOverrides`. Run `unit2nix --check-overrides` to see what's covered.

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

## Cross-compilation

Cargo resolves different dep trees per target, so each target gets its own plan:

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

## Testing

For workspaces, use `--workspace` to capture dev-dependencies for **all** members:

```bash
cargo unit2nix --workspace
nix build .#test.check.my-crate
```

All public test attrs (`test.workspaceMembers`, `test.allWorkspaceMembers`, and
`test.check`) use per-member test graphs. Only the selected member gets
`devDependencies`, so a dev-dependency cycle in one workspace member does not
poison unrelated members or the aggregate test attr.

For single-crate projects, `--include-dev` is sufficient:

```bash
cargo unit2nix --include-dev
nix build .#test.check.my-crate
```

Fast Rust-side CLI integration coverage lives in `tests/cli.rs`. It exercises
real `unit2nix` and `cargo-unit2nix` binaries against `sample_workspace`,
covering build-plan generation, `--stdout`, `--workspace`, `--members`,
`--check-overrides --json`, fingerprint skips, fast flag validation, and the
user-visible `--build-std` path on a minimal `#![no_std]` fixture.

## Git dependencies

Git deps are prefetched at generation time so builds stay pure. In auto mode, place a `crate-hashes.json` at the root (same format as crate2nix):

```json
{
  "https://github.com/user/repo?branch=main#crate-name@1.0.0": "sha256-..."
}
```

## CLI

```
unit2nix [OPTIONS]

  --manifest-path <PATH>    Path to Cargo.toml [default: ./Cargo.toml]
  --features <FEATURES>     Comma-separated features to enable
  --all-features            Enable all features
  --no-default-features     Disable default features
  --bin <NAME>              Build a specific binary target
  -p, --package <NAME>      Build a specific package
  --members <NAMES>         Workspace members to include (comma-separated)
  --target <TRIPLE>         Cross-compilation target
  --include-dev             Include dev-dependencies (for nix test support)
  --workspace               Resolve all workspace members + dev-deps (implies --include-dev)
  -o, --output <FILE>       Output file [default: build-plan.json]
  --stdout                  Write to stdout
  --force                   Regenerate even if inputs haven't changed
  --check-overrides         Report -sys crate override coverage
  --json                    Machine-readable output (with --check-overrides)
  --no-check                Skip override check after generation
```

Also available as `cargo unit2nix`.

## vs crate2nix

| | unit2nix | crate2nix |
|---|---|---|
| Resolver | Cargo itself (unit graph) | Reimplemented in Rust |
| Platform filtering | Cargo does it | `cfg()` evaluator in Nix |
| Cross-compilation | One JSON per target | One JSON, filtered at eval |
| Stability | Nightly (needs `--unit-graph`) | Stable Rust |

unit2nix requires nightly Cargo for `--unit-graph`.

## Tested on

| Project | Crates | Notes |
|---|---|---|
| [ripgrep](https://github.com/BurntSushi/ripgrep) | 34 | Pure Rust, zero overrides |
| [fd](https://github.com/sharkdp/fd) | 59 | jemalloc covered by built-ins |
| [bat](https://github.com/sharkdp/bat) | 168 | libgit2, libz, onig auto-covered |
| [nushell](https://github.com/nushell/nushell) | 519 | sqlite, ring auto-covered |
| Private workspace | 457 | Production build |

## Development

```bash
cargo test           # unit tests
nix flake check      # sample builds, clippy, overlay/module smoke tests,
                     # override coverage, fd/bat/ripgrep/nushell validation,
                     # cross-compilation, NixOS VM integration tests
```

## Requirements

- Nightly Rust (`--unit-graph` is unstable)
- Nix with flakes

## License

MIT
