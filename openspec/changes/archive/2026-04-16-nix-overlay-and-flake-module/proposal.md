# Nixpkgs Overlay and Flake-Parts Module

## Problem

Today, consumers wire unit2nix into their flakes by calling functions from `unit2nix.lib.${system}`:

```nix
unit2nix.lib.${system}.buildFromUnitGraph {
  inherit pkgs;
  src = ./.;
  resolvedJson = ./build-plan.json;
};
```

This has friction:

1. **System threading** — every call needs `${system}` and a manually-constructed `pkgs`
2. **Boilerplate** — consumers copy ~60 lines from the template, plumbing `packages`, `checks`, `devShells` by hand
3. **No composability with nixpkgs** — can't use `pkgs.unit2nix.buildFromUnitGraph` in NixOS modules, overlays, or other nixpkgs-integrated tooling
4. **No convention** — every consumer reinvents the same wiring: expose `packages.default`, add clippy/test checks, wire dev shell

## Solution

Two new flake outputs:

1. **`overlays.default`** — a nixpkgs overlay that puts `buildFromUnitGraph`, `buildFromUnitGraphAuto`, and the `unit2nix` CLI on `pkgs.unit2nix`. Eliminates system-threading — anywhere you have `pkgs`, you have the builder.

2. **`flakeModules.default`** — a flake-parts module that wires convention: declare `src` and `resolvedJson`, get `packages.default`, `checks.clippy`, `checks.tests`, `devShells.default`, and `apps.update-plan` auto-generated.

## Value

- **Less boilerplate** — a flake-parts consumer declares ~10 lines instead of ~60
- **Composable** — overlay works in NixOS modules, other overlays, devShells
- **Convention over configuration** — sensible defaults with opt-out escape hatches
- **Backward compatible** — existing `lib.${system}.*` API is unchanged
