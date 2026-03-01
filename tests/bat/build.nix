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

  ws = buildFromUnitGraph {
    inherit pkgs;
    src = batSrc;
    resolvedJson = ./build-plan.json;
    skipStalenessCheck = true;
    defaultCrateOverrides = pkgs.defaultCrateOverrides // {
      # libgit2-sys needs the libgit2 C library and pkg-config to find it.
      # The `links = "git2"` field tells buildRustCrate this crate provides
      # the -lgit2 native dependency.
      libgit2-sys = attrs: {
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = [ pkgs.libgit2 ];
        # Tell the build script to use the system libgit2 instead of building
        # its vendored copy (which would need cmake + C compiler in sandbox).
        LIBGIT2_NO_VENDOR = "1";
      };

      # libz-sys needs zlib. The build script can either vendor or use system.
      libz-sys = attrs: {
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = [ pkgs.zlib ];
        LIBZ_SYS_STATIC = "0";
      };

      # bat's own build script processes syntax/theme assets from the source tree.
      # It needs the assets/ directory available and the BAHT_ASSETS_GEN_DIR set.
      bat = attrs: {
        # bat's build.rs looks for assets relative to CARGO_MANIFEST_DIR
        # which buildRustCrate sets correctly.
        buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
          pkgs.darwin.apple_sdk.frameworks.Security
        ];
      };
    };
  };
in
ws.workspaceMembers."bat".build
