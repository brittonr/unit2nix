# Build fd from a unit2nix build plan.
# This validates unit2nix against a ~59-crate workspace with:
#   - tikv-jemalloc-sys (-sys crate with vendored C build)
#   - Popular file-finding CLI tool
#
# Usage:
#   nix-build tests/fd/build.nix
#   ./result/bin/fd --version
{
  pkgs ? import <nixpkgs> { },
}:
let
  buildFromUnitGraph = import ../../lib/build-from-unit-graph.nix;

  fdSrc = pkgs.fetchFromGitHub {
    owner = "sharkdp";
    repo = "fd";
    rev = "c60d0076db720529ba4df167f6130297f4469d11";
    hash = "sha256-gi54cDL3qm01qaV9vlMWjyYbLUxicPFIAjji3DxXkAs=";
  };

  # With unit2nix's three-layer override merge (nixpkgs + unit2nix built-ins + user),
  # fd needs no explicit overrides:
  #   - tikv-jemalloc-sys: covered by unit2nix built-in overrides
  ws = buildFromUnitGraph {
    inherit pkgs;
    src = fdSrc;
    resolvedJson = ./build-plan.json;
    skipStalenessCheck = true;
  };
in
ws.workspaceMembers."fd-find".build
