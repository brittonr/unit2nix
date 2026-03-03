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

  ws = buildFromUnitGraph {
    inherit pkgs;
    src = fdSrc;
    resolvedJson = ./build-plan.json;
    skipStalenessCheck = true;
    defaultCrateOverrides = pkgs.defaultCrateOverrides // {
      # tikv-jemalloc-sys vendors and builds jemalloc from source.
      # It needs a C compiler (provided by buildRustCrate's stdenv)
      # and make for the vendored build.
      tikv-jemalloc-sys = attrs: {
        nativeBuildInputs = [ pkgs.makeWrapper ];
        buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
          pkgs.darwin.apple_sdk.frameworks.Security
        ];
      };
    };
  };
in
ws.workspaceMembers."fd-find".build
