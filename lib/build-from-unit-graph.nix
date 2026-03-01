# Build a Rust workspace from unit2nix's pre-resolved JSON.
#
# All dependency resolution, feature expansion, and platform filtering is done
# by Cargo via `--unit-graph`. This file does zero resolution — it wires
# pre-resolved data to buildRustCrate.
#
# Usage:
#   let
#     ws = import ./build-from-unit-graph.nix {
#       inherit pkgs src;
#       resolvedJson = ./build-plan.json;
#     };
#   in ws.workspaceMembers.my-crate.build

{
  pkgs,
  lib ? pkgs.lib,
  # Workspace source root
  src,
  # Path to the pre-resolved JSON file (from unit2nix)
  resolvedJson,
  # Optional: buildRustCrate override
  buildRustCrateForPkgs ? pkgs: pkgs.buildRustCrate,
  # Optional: default crate overrides
  defaultCrateOverrides ? pkgs.defaultCrateOverrides,
}:

let
  resolved = builtins.fromJSON (builtins.readFile resolvedJson);
  fetchSource = import ./fetch-source.nix { inherit pkgs src; };

  # Build the recursive crate set for a given pkgs instance.
  mkBuiltByPackageIdByPkgs =
    cratePkgs:
    let
      buildRustCrate =
        let
          base = buildRustCrateForPkgs cratePkgs;
        in
        if defaultCrateOverrides != pkgs.defaultCrateOverrides then
          base.override { defaultCrateOverrides = defaultCrateOverrides; }
        else
          base;

      self = {
        # Each crate keyed by its full package ID
        crates = lib.mapAttrs (
          packageId: _crateInfo: buildCrate self cratePkgs buildRustCrate packageId
        ) resolved.crates;

        # For proc-macro / build-dep host platform builds
        build = mkBuiltByPackageIdByPkgs cratePkgs.buildPackages;
      };
    in
    self;

  # Build a single crate derivation.
  buildCrate =
    self: cratePkgs: buildRustCrate: packageId:
    let
      crateInfo = resolved.crates.${packageId};

      # Resolve a dependency to its derivation.
      # Proc-macro deps must be built for the host platform.
      depDrv =
        dep:
        let
          depInfo = resolved.crates.${dep.packageId} or null;
          isProcMacro = depInfo != null && (depInfo.procMacro or false);
        in
        if isProcMacro then
          self.build.crates.${dep.packageId}
        else
          self.crates.${dep.packageId};

      dependencies = map depDrv (crateInfo.dependencies or [ ]);
      buildDependencies = map depDrv (crateInfo.buildDependencies or [ ]);

      # Compute crate renames: when externCrateName differs from the dep's crateName
      allDeps = (crateInfo.dependencies or [ ]) ++ (crateInfo.buildDependencies or [ ]);
      renamedDeps = builtins.filter (
        dep:
        let
          depInfo = resolved.crates.${dep.packageId} or null;
          depCrateName = if depInfo != null then
            builtins.replaceStrings [ "-" ] [ "_" ] depInfo.crateName
          else
            dep.externCrateName;
        in
        dep.externCrateName != depCrateName
      ) allDeps;

      crateRenames =
        let
          grouped = lib.groupBy (dep: (resolved.crates.${dep.packageId}).crateName) renamedDeps;
          versionAndRename = dep: {
            rename = dep.externCrateName;
            version = (resolved.crates.${dep.packageId}).version;
          };
        in
        lib.mapAttrs (_name: builtins.map versionAndRename) grouped;

      crateSrc = fetchSource crateInfo;
    in
    buildRustCrate (
      {
        crateName = crateInfo.crateName;
        version = crateInfo.version;
        edition = crateInfo.edition or "2021";
        src = crateSrc;
        inherit dependencies buildDependencies crateRenames;
        features = crateInfo.features or [ ];
        procMacro = crateInfo.procMacro or false;
        crateBin = crateInfo.crateBin or [ ];
      }
      // lib.optionalAttrs ((crateInfo.sha256 or null) != null) {
        sha256 = crateInfo.sha256;
      }
      // lib.optionalAttrs ((crateInfo.build or null) != null) {
        build = crateInfo.build;
      }
      // lib.optionalAttrs ((crateInfo.libPath or null) != null) {
        libPath = crateInfo.libPath;
      }
      // lib.optionalAttrs ((crateInfo.libName or null) != null) {
        libName = crateInfo.libName;
      }
      // lib.optionalAttrs ((crateInfo.links or null) != null) {
        links = crateInfo.links;
      }
      // lib.optionalAttrs ((crateInfo.libCrateTypes or [ ]) != [ ]) {
        type = crateInfo.libCrateTypes;
      }
    );

  builtCrates = mkBuiltByPackageIdByPkgs pkgs;

  # Find workspace member package IDs by matching local source paths
  # against crate names in the JSON.
  workspaceMemberIds = lib.filterAttrs (
    _packageId: crateInfo:
    let
      source = crateInfo.source or null;
      sourceType = if source == null then "local" else source.type or "local";
    in
    sourceType == "local"
  ) resolved.crates;

in
{
  workspaceMembers = lib.mapAttrs (
    packageId: crateInfo: {
      inherit packageId;
      build = builtCrates.crates.${packageId};
    }
  ) workspaceMemberIds;

  rootCrate =
    let
      roots = resolved.roots or [ ];
      rootId = if roots != [ ] then builtins.head roots else null;
    in
    if rootId != null then
      {
        packageId = rootId;
        build = builtCrates.crates.${rootId};
      }
    else
      null;

  allWorkspaceMembers = pkgs.symlinkJoin {
    name = "all-workspace-members";
    paths = lib.mapAttrsToList (
      packageId: _crateInfo: builtCrates.crates.${packageId}
    ) workspaceMemberIds;
  };

  inherit resolved;
  inherit builtCrates;
}
