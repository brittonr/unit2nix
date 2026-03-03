# Build a Rust workspace using the unit2nix Nix plugin.
#
# Requires: nix --option plugin-files /path/to/libunit2nix_plugin.so
#
# Usage:
#   let
#     ws = import ./plugin.nix {
#       inherit pkgs;
#       src = ./.;  # workspace root with Cargo.toml + Cargo.lock
#     };
#   in ws.allWorkspaceMembers

{
  pkgs,
  lib ? pkgs.lib,
  src,
  buildRustCrateForPkgs ? pkgs: pkgs.buildRustCrate,
  defaultCrateOverrides ? null,
  extraCrateOverrides ? {},
  clippyArgs ? [],
  members ? null,
  # Plugin-specific options
  target ? null,
  includeDev ? false,
  features ? null,
  allFeatures ? false,
  noDefaultFeatures ? false,
  bin ? null,
  package ? null,
}:

let
  # Call the native builtin to resolve the workspace
  resolved = builtins.resolveUnitGraph ({
    manifestPath = "${src}/Cargo.toml";
  } // lib.optionalAttrs (target != null) {
    inherit target;
  } // lib.optionalAttrs includeDev {
    inherit includeDev;
  } // lib.optionalAttrs (features != null) {
    inherit features;
  } // lib.optionalAttrs allFeatures {
    inherit allFeatures;
  } // lib.optionalAttrs noDefaultFeatures {
    inherit noDefaultFeatures;
  } // lib.optionalAttrs (bin != null) {
    inherit bin;
  } // lib.optionalAttrs (package != null) {
    inherit package;
  } // lib.optionalAttrs (members != null) {
    members = builtins.concatStringsSep "," members;
  });

in import ./build-from-unit-graph.nix {
  inherit
    pkgs
    lib
    src
    buildRustCrateForPkgs
    defaultCrateOverrides
    extraCrateOverrides
    clippyArgs
    members
    ;
  # Pass the resolved attrset directly — no JSON file needed
  resolvedData = resolved;
  # Skip staleness check — the plugin resolves live from Cargo.toml
  skipStalenessCheck = true;
}
