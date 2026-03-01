# buildRustPackage build of sample_workspace — for benchmarking against unit2nix.
#
# buildRustPackage builds the entire workspace in a single derivation.
# Any source or Cargo.lock change triggers a full rebuild.
{
  pkgs ? import <nixpkgs> { },
}:
let
  workspace = pkgs.rustPlatform.buildRustPackage {
    pname = "sample-workspace";
    version = "0.1.0";
    src = ../sample_workspace;
    cargoLock.lockFile = ../sample_workspace/Cargo.lock;
  };
in
{
  inherit workspace;
  sample-bin = workspace;
}
