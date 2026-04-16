# Nixpkgs Overlay and Flake-Parts Module — Design

## Overview

Two layers: a nixpkgs overlay (low-level, composable) and a flake-parts module (high-level, convention-driven). The overlay is a dependency of the module — the module applies the overlay and then calls functions from it.

## 1. Nixpkgs Overlay

### Shape

```nix
# overlays.default
final: prev: {
  unit2nix = {
    # The CLI tool
    cli = final.callPackage ./nix/package.nix { ... };

    # Builder functions (no system threading — pkgs is `final`)
    buildFromUnitGraph = args: import ./lib/build-from-unit-graph.nix ({ pkgs = final; } // args);
    buildFromUnitGraphAuto = args: import ./lib/auto.nix ({ pkgs = final; unit2nix = final.unit2nix.cli; } // args);

    # Crate overrides
    crateOverrides = (import ./lib/crate-overrides.nix { pkgs = final; }).overrides;
    isKnownNoOverride = (import ./lib/crate-overrides.nix { pkgs = final; }).isKnownNoOverride;
  };
}
```

### Usage

```nix
# Consumer's flake.nix
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
      packages.x86_64-linux.default = ws.workspaceMembers.my-crate.build;
    };
}
```

### Key decisions

- **Namespace**: `pkgs.unit2nix.*` — avoids polluting the top-level pkgs namespace. Similar to how `pkgs.haskellPackages.*` works.
- **`pkgs` is implicit**: `buildFromUnitGraph` no longer needs `pkgs` passed — it uses `final` from the overlay. Users can still override via the `pkgs` arg if needed (e.g., for cross-compilation with `pkgsCross`).
- **`cli` not `unit2nix`**: The CLI is at `pkgs.unit2nix.cli` (not `pkgs.unit2nix`) to avoid collision with the attrset.
- **Self-contained**: The overlay carries its own source — `./lib/*.nix` and `./nix/package.nix` are captured at flake eval time. No runtime dependency on the flake input.

## 2. Flake-Parts Module

### Dependencies

The flake.nix adds `flake-parts` as an input. The module is in `flake-modules/default.nix`.

**Note**: flake-parts is only needed by consumers who use the module. The overlay has zero extra dependencies.

### Module options

```nix
options.unit2nix = {
  enable = lib.mkEnableOption "unit2nix Rust build integration";

  src = lib.mkOption {
    type = lib.types.path;
    description = "Workspace source root";
  };

  resolvedJson = lib.mkOption {
    type = lib.types.nullOr lib.types.path;
    default = null;
    description = "Path to build-plan.json (null = use auto mode)";
  };

  workspaceDir = lib.mkOption {
    type = lib.types.nullOr lib.types.path;
    default = null;
    description = "Parent directory for projects with external path deps";
  };

  defaultPackage = lib.mkOption {
    type = lib.types.nullOr lib.types.str;
    default = null;
    description = "Workspace member name for packages.default (null = allWorkspaceMembers)";
  };

  members = lib.mkOption {
    type = lib.types.nullOr (lib.types.listOf lib.types.str);
    default = null;
    description = "Workspace member filter (null = all)";
  };

  extraCrateOverrides = lib.mkOption {
    type = lib.types.attrs;
    default = {};
    description = "Project-specific -sys crate overrides";
  };

  checks = {
    clippy = lib.mkEnableOption "clippy check" // { default = true; };
    tests = lib.mkEnableOption "test check" // { default = true; };
    overrides = lib.mkEnableOption "override coverage check" // { default = false; };
  };

  devShell = {
    enable = lib.mkEnableOption "dev shell with unit2nix + cargo + rustc" // { default = true; };
    extraPackages = lib.mkOption {
      type = lib.types.listOf lib.types.package;
      default = [];
      description = "Extra packages in the dev shell";
    };
  };

  rustToolchain = lib.mkOption {
    type = lib.types.nullOr lib.types.package;
    default = null;
    description = "Custom Rust toolchain (e.g. from rust-overlay)";
  };
};
```

### What it wires

When `unit2nix.enable = true`:

| Output | Value |
|--------|-------|
| `packages.default` | `ws.workspaceMembers.<defaultPackage>.build` or `ws.allWorkspaceMembers` |
| `packages.<name>` | One per workspace member → `ws.workspaceMembers.<name>.build` |
| `checks.clippy` | `ws.clippy.allWorkspaceMembers` (when enabled) |
| `checks.tests` | `pkgs.runCommand` executing all `ws.test.check.*` (when enabled) |
| `devShells.default` | Shell with `unit2nix.cli`, `cargo`, `rustc`, `rust-analyzer` + extras |
| `apps.update-plan` | `unit2nix -o build-plan.json` (manual mode only) |

### Usage

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
        resolvedJson = ./build-plan.json;
        defaultPackage = "my-bin";
      };
    };
}
```

### Auto mode

When `resolvedJson = null`, the module uses `buildFromUnitGraphAuto` instead. The `apps.update-plan` output is omitted (not needed).

## 3. What does NOT change

- `lib.${system}.buildFromUnitGraph` — stays as-is
- `lib.${system}.buildFromUnitGraphAuto` — stays as-is
- `templates.default` — stays, but updated docs mention the module alternative
- All existing Nix files in `lib/` — no changes needed
- Rust CLI — no changes

## 4. File layout

```
flake.nix              — adds overlays.default, flakeModules.default outputs
nix/overlay.nix        — overlay implementation
flake-modules/
  default.nix          — flake-parts module implementation
```

## 5. Testing

- **Overlay smoke test**: Nix check that imports nixpkgs with overlay, builds sample workspace via `pkgs.unit2nix.buildFromUnitGraph`
- **Module smoke test**: Minimal flake-parts consumer that uses the module, builds `packages.default`
- **Backward compat**: Existing 16 checks still pass unchanged
- **Template**: Updated with overlay alternative in comments

## 6. Dependency considerations

- **Overlay**: Zero new dependencies. Pure nixpkgs overlay.
- **Module**: `flake-parts` becomes an input of the unit2nix flake. It's dev-time only — consumers who don't use the module never evaluate it. flake-parts is the de facto standard for flake modules (~3k dependents on FlakeHub).
