# Crane build of sample_workspace — for benchmarking against unit2nix.
#
# Crane builds the whole workspace as a single derivation (deps layer + source layer).
# Any Cargo.lock change rebuilds the entire deps layer.
{
  pkgs ? import <nixpkgs> { },
  craneSrc ? builtins.fetchGit {
    url = "https://github.com/ipetkov/crane";
    rev = "8525580bc0316c39dbfa18bd09a1331e98c9e463";
  },
}:
let
  crane = import craneSrc { inherit pkgs; };

  src = ../sample_workspace;

  commonArgs = {
    inherit src;
    pname = "sample-workspace";
    version = "0.1.0";
    strictDeps = true;
  };

  # Deps-only layer (rebuilt on any Cargo.lock change)
  cargoArtifacts = crane.buildDepsOnly commonArgs;

  # Full workspace build
  workspace = crane.buildPackage (commonArgs // {
    inherit cargoArtifacts;
  });
in
{
  inherit workspace cargoArtifacts;
  # Expose individual binary for apples-to-apples comparison
  sample-bin = workspace;
}
