# Build nushell from a unit2nix build plan.
# This validates unit2nix against a ~519-crate workspace with:
#   - 29 workspace members (largest multi-root workspace tested)
#   - libsqlite3-sys, ring (-sys crates with native builds)
#   - Proc-macro dependencies
#   - Complex feature matrix
#
# Usage:
#   nix-build tests/nushell/build.nix
#   ./result/bin/nu --version
{
  pkgs ? import <nixpkgs> { },
}:
let
  buildFromUnitGraph = import ../../lib/build-from-unit-graph.nix;

  nushellSrc = pkgs.fetchFromGitHub {
    owner = "nushell";
    repo = "nushell";
    rev = "14f030b9836e87f9436955c677aefe493dbec444";
    hash = "sha256-OBbm6x9ohOKcfBuI2ElcXN1JAVbaI229Gec6+tF+BxY=";
  };

  # With unit2nix's three-layer override merge (nixpkgs + unit2nix built-ins + user),
  # nushell needs no explicit overrides:
  #   - libsqlite3-sys: covered by nixpkgs defaultCrateOverrides
  #   - ring: covered by unit2nix built-in overrides
  ws = buildFromUnitGraph {
    inherit pkgs;
    src = nushellSrc;
    resolvedJson = ./build-plan.json;
    skipStalenessCheck = true;
  };
in
ws.workspaceMembers."nu".build
