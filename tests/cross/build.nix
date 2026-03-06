# Cross-compilation validation: generate an aarch64 build plan from the
# sample workspace and build it with pkgsCross.aarch64-multiplatform.
#
# Validates:
#   - unit2nix --target produces a correct cross build plan
#   - buildFromUnitGraph routes proc-macros/build-scripts to build platform
#   - Output binary is ELF aarch64
#
# Usage:
#   nix build .#checks.x86_64-linux.validate-cross-aarch64
{
  pkgs ? import <nixpkgs> { },
  unit2nix,
}:
let
  lib = pkgs.lib;

  buildFromUnitGraph = import ../../lib/build-from-unit-graph.nix;

  crossPkgs = pkgs.pkgsCross.aarch64-multiplatform;

  sampleSrc = ../../sample_workspace;

  # Vendor crate sources so the IFD derivation needs no network access.
  vendor = import ../../lib/vendor.nix {
    inherit pkgs lib;
    cargoLock = sampleSrc + "/Cargo.lock";
    crateHashesJson = null;
  };

  # Generate a cross build plan via IFD.
  # Uses --target so cargo filters dependencies for aarch64.
  crossPlan = pkgs.runCommand "unit2nix-cross-plan" {
    nativeBuildInputs = [ unit2nix ];
    preferLocalBuild = true;
  } ''
    export CARGO_HOME=$(mktemp -d)
    mkdir -p "$CARGO_HOME"
    cp ${vendor.cargoConfig} "$CARGO_HOME/config.toml"

    cp -r ${sampleSrc} source
    chmod -R u+w source
    cd source

    unit2nix --target aarch64-unknown-linux-gnu -o "$out" --no-check
  '';

  # Build the sample workspace for aarch64 using the cross build plan.
  ws = buildFromUnitGraph {
    pkgs = crossPkgs;
    src = sampleSrc;
    resolvedJson = crossPlan;
    skipStalenessCheck = true;
  };

  sampleBin = ws.workspaceMembers."sample-bin".build;

in
# Validate: the output binary is an aarch64 ELF executable.
pkgs.runCommand "validate-cross-aarch64" {
  nativeBuildInputs = [ pkgs.file ];
} ''
  arch="$(file -b ${sampleBin}/bin/sample-bin)"
  echo "Binary architecture: $arch"

  if echo "$arch" | grep -q "ELF 64-bit.*ARM aarch64"; then
    echo "PASS: sample-bin is aarch64"
  else
    echo "FAIL: expected aarch64 ELF, got: $arch"
    exit 1
  fi

  # Also verify all workspace members built (the ws evaluation succeeds
  # only if proc-macros and build scripts compiled for the build platform).
  echo "All workspace members: ${ws.allWorkspaceMembers}"
  echo "PASS: all workspace members built successfully"

  touch "$out"
''
