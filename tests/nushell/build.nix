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

  ws = buildFromUnitGraph {
    inherit pkgs;
    src = nushellSrc;
    resolvedJson = ./build-plan.json;
    skipStalenessCheck = true;
    defaultCrateOverrides = pkgs.defaultCrateOverrides // {
      # libsqlite3-sys: use system sqlite instead of vendored build.
      libsqlite3-sys = attrs: {
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = [ pkgs.sqlite ];
        SQLITE3_LIB_DIR = "${pkgs.sqlite.out}/lib";
        SQLITE3_INCLUDE_DIR = "${pkgs.sqlite.dev}/include";
      };

      # rmcp uses env!("CARGO_CRATE_NAME") at compile time, which cargo sets
      # but buildRustCrate doesn't. Provide it manually.
      rmcp = attrs: {
        CARGO_CRATE_NAME = "rmcp";
      };

      # nu's build script reads CARGO_CFG_FEATURE to embed enabled features.
      # buildRustCrate doesn't set CARGO_CFG_* vars — provide it manually.
      nu = attrs: {
        CARGO_CFG_FEATURE = builtins.concatStringsSep "," (attrs.features or [ ]);
      };

      # ring: build script compiles pregenerated assembly via cc.
      # Do NOT set RING_PREGENERATE_ASM — that tries to regenerate asm
      # files and fails when the pregenerated/ dir already exists.
      ring = attrs: {
        nativeBuildInputs = [ pkgs.perl ];
        buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
          pkgs.darwin.apple_sdk.frameworks.Security
        ];
      };
    };
  };
in
ws.workspaceMembers."nu".build
