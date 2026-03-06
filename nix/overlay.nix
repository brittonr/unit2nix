# Nixpkgs overlay that puts unit2nix tools on pkgs.unit2nix.
#
# Usage:
#   pkgs = import nixpkgs {
#     system = "x86_64-linux";
#     overlays = [ unit2nix.overlays.default ];
#   };
#   ws = pkgs.unit2nix.buildFromUnitGraph {
#     src = ./.;
#     resolvedJson = ./build-plan.json;
#   };
#
# pkgs is implicit — buildFromUnitGraph and buildFromUnitGraphAuto default
# to the overlay's `final` pkgs. Pass `pkgs` explicitly to override
# (e.g., for cross-compilation with pkgsCross).

{ self }:

final: prev: {
  unit2nix = {
    # The CLI tool (wrapped with cargo, rustc, nix-prefetch-git on PATH)
    cli = final.callPackage ./package.nix {
      src = self;
      cargoLockFile = self + "/Cargo.lock";
    };

    # Build a workspace from a pre-resolved build plan JSON.
    # pkgs defaults to `final` — callers don't need to pass it.
    buildFromUnitGraph =
      args:
      import ../lib/build-from-unit-graph.nix ({ pkgs = final; } // args);

    # Auto mode: generate build plan via IFD (no manual step needed).
    # pkgs defaults to `final`, unit2nix defaults to the overlay's CLI.
    buildFromUnitGraphAuto =
      args:
      import ../lib/auto.nix ({
        pkgs = final;
        unit2nix = final.unit2nix.cli;
      } // args);

    # Built-in crate overrides (ring, tikv-jemalloc-sys, etc.)
    crateOverrides = (import ../lib/crate-overrides.nix { pkgs = final; }).overrides;

    # Check if a crate is known to not need overrides
    isKnownNoOverride = (import ../lib/crate-overrides.nix { pkgs = final; }).isKnownNoOverride;
  };
}
