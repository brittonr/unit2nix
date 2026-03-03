# Build bat from a unit2nix build plan.
# This validates unit2nix against a 168-crate workspace with:
#   - -sys crates (libgit2-sys, libz-sys) requiring native C libraries
#   - Custom build script (build/main.rs for syntax/theme processing)
#   - Proc-macro dependencies
#
# Usage:
#   nix-build tests/bat/build.nix
#   ./result/bin/bat --version
{
  pkgs ? import <nixpkgs> { },
}:
let
  buildFromUnitGraph = import ../../lib/build-from-unit-graph.nix;

  batSrc = pkgs.fetchFromGitHub {
    owner = "sharkdp";
    repo = "bat";
    rev = "908b1f22d1a45f93ac5400b3017df11e459d3013";
    hash = "sha256-B22j2UvDj1d6Jdst/PtMAu2Z9ISz+JNiphQyjZNzX+Y=";
  };

  # With unit2nix's three-layer override merge (nixpkgs + unit2nix built-ins + user),
  # bat needs no explicit overrides:
  #   - libgit2-sys, libz-sys: covered by nixpkgs defaultCrateOverrides
  #   - onig_sys: covered by unit2nix built-in overrides
  ws = buildFromUnitGraph {
    inherit pkgs;
    src = batSrc;
    resolvedJson = ./build-plan.json;
    skipStalenessCheck = true;
    extraCrateOverrides = {
      # bat's own build script processes syntax/theme assets from the source tree.
      bat = attrs: {
        buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
          pkgs.darwin.apple_sdk.frameworks.Security
        ];
      };
    };
  };
in
ws.workspaceMembers."bat".build
