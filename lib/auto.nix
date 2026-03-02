# Auto-build: generate build-plan.json via IFD and build with buildFromUnitGraph.
#
# Vendors crate sources from Cargo.lock, runs unit2nix in a sandboxed
# derivation, and imports the result at eval time (IFD). No manual
# regeneration step needed.
#
# Requires: IFD enabled (default in Nix; disabled on Hydra).
#
# Usage:
#   let
#     ws = import ./auto.nix {
#       inherit pkgs;
#       unit2nix = <unit2nix package>;
#       src = ./.;
#     };
#   in ws.workspaceMembers.my-crate.build

{
  pkgs,
  lib ? pkgs.lib,
  # The unit2nix Nix package (with cargo/rustc on PATH)
  unit2nix,
  # Workspace source root
  src,
  # Optional: buildRustCrate override (forwarded to buildFromUnitGraph)
  buildRustCrateForPkgs ? pkgs: pkgs.buildRustCrate,
  # Optional: default crate overrides (forwarded to buildFromUnitGraph)
  defaultCrateOverrides ? pkgs.defaultCrateOverrides,
}:

let
  cargoLockPath = src + "/Cargo.lock";
  crateHashesPath = src + "/crate-hashes.json";

  vendor = import ./vendor.nix {
    inherit pkgs lib;
    cargoLock = cargoLockPath;
    crateHashesJson =
      if builtins.pathExists crateHashesPath
      then crateHashesPath
      else null;
  };

  # Generate build-plan.json in a sandboxed derivation.
  # Cargo uses vendored sources (no network access needed).
  generatedPlan = pkgs.runCommand "unit2nix-build-plan" {
    nativeBuildInputs = [ unit2nix ];
    preferLocalBuild = true;
  } ''
    # Set up vendored cargo home
    export CARGO_HOME=$(mktemp -d)
    mkdir -p "$CARGO_HOME"
    cp ${vendor.cargoConfig} "$CARGO_HOME/config.toml"

    # Copy source (unit2nix needs to read Cargo.toml, Cargo.lock, etc.)
    cp -r ${src} source
    chmod -R u+w source
    cd source

    unit2nix --manifest-path ./Cargo.toml -o "$out"
  '';

in
import ./build-from-unit-graph.nix {
  inherit pkgs lib src buildRustCrateForPkgs defaultCrateOverrides;
  resolvedJson = generatedPlan;
  skipStalenessCheck = true;
}
