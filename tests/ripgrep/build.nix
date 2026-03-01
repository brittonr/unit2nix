# Build ripgrep from a unit2nix build plan.
# This validates unit2nix against a real-world ~34-crate workspace.
#
# Usage:
#   nix-build tests/ripgrep/build.nix
#   ./result/bin/rg --version
{
  pkgs ? import <nixpkgs> { },
}:
let
  buildFromUnitGraph = import ../../lib/build-from-unit-graph.nix;

  ripgrepSrc = pkgs.fetchFromGitHub {
    owner = "BurntSushi";
    repo = "ripgrep";
    rev = "4519153e5e461527f4bca45b042fff45c4ec6fb9";
    hash = "sha256-sU3qM0qgiEPdBSOVXp/5Nt0m8g8xthuvYO+EPAm/MLQ=";
  };

  ws = buildFromUnitGraph {
    inherit pkgs;
    src = ripgrepSrc;
    resolvedJson = ./build-plan.json;
    # Pinned source with separate Cargo.lock — skip staleness check
    skipStalenessCheck = true;
  };
in
ws.workspaceMembers."ripgrep".build
