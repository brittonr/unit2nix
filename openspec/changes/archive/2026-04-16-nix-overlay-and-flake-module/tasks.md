## 1. Nixpkgs overlay

- [x] 1.1 Create `nix/overlay.nix` — `final: prev:` overlay that puts `unit2nix.cli`, `unit2nix.buildFromUnitGraph`, `unit2nix.buildFromUnitGraphAuto`, `unit2nix.crateOverrides`, `unit2nix.isKnownNoOverride` on `final.unit2nix`
- [x] 1.2 `buildFromUnitGraph` in overlay defaults `pkgs = final` so callers don't need to pass it
- [x] 1.3 `buildFromUnitGraphAuto` in overlay defaults `pkgs = final` and `unit2nix = final.unit2nix.cli`
- [x] 1.4 Add `overlays.default = import ./nix/overlay.nix { inherit self; }` to `flake.nix` outputs

## 2. Flake-parts module

- [x] 2.1 Add `flake-parts` input to `flake.nix`
- [x] 2.2 Create `flake-modules/default.nix` — flake-parts module with `options.unit2nix` and `config` wiring
- [x] 2.3 Module options: `enable`, `src`, `resolvedJson`, `workspaceDir`, `defaultPackage`, `members`, `extraCrateOverrides`, `checks.{clippy, tests, overrides}`, `devShell.{enable, extraPackages}`, `rustToolchain`
- [x] 2.4 Module applies the overlay to nixpkgs automatically (via `config.nixpkgs.overlays` or perSystem overlay)
- [x] 2.5 Module wires `packages.default` — uses `defaultPackage` member if set, otherwise `allWorkspaceMembers`
- [x] 2.6 Module wires `packages.<name>` for each workspace member
- [x] 2.7 Module wires `checks.unit2nix-clippy` when `checks.clippy = true`
- [x] 2.8 Module wires `checks.unit2nix-tests` when `checks.tests = true`
- [x] 2.9 Module wires `checks.unit2nix-overrides` when `checks.overrides = true`
- [x] 2.10 Module wires `devShells.default` when `devShell.enable = true` — includes `unit2nix.cli`, `cargo`, `rustc`, `rust-analyzer`, plus `devShell.extraPackages`
- [x] 2.11 Module wires `apps.update-plan` when `resolvedJson != null` (manual mode)
- [x] 2.12 Module uses `buildFromUnitGraphAuto` when `resolvedJson = null` (auto mode)
- [x] 2.13 Module forwards `rustToolchain` to auto mode when set

## 3. Flake.nix integration

- [x] 3.1 Add `flakeModules.default` to flake.nix outputs
- [x] 3.2 Ensure existing `lib`, `packages`, `checks`, `devShells` outputs are unchanged
- [x] 3.3 Verify all 16 existing checks still pass (18 total now — 16 original + 2 new)

## 4. Tests

- [x] 4.1 Overlay smoke test: nix check that builds sample workspace via `pkgs.unit2nix.buildFromUnitGraph` (no `lib.${system}` — uses overlay)
- [x] 4.2 Flake-parts module test: minimal flake in `tests/flake-parts/` that uses the module, verify `nix build` produces working binary
- [x] 4.3 Backward compat: all 16 existing checks pass without modification

## 5. Documentation

- [x] 5.1 Update README with overlay usage example
- [x] 5.2 Update README with flake-parts module usage example
- [x] 5.3 Update template `flake.nix` with overlay alternative in comments
- [x] 5.4 Update napkin with session notes
