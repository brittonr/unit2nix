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
  # Skip the Cargo.lock staleness check (default: false).
  # Set to true when src filtering strips Cargo.lock or for other edge cases.
  skipStalenessCheck ? false,
}:

let
  resolved = builtins.fromJSON (builtins.readFile resolvedJson);

  # Staleness check: verify build-plan.json matches the current Cargo.lock.
  # Skipped when: check is disabled, hash is absent (old unit2nix), or Cargo.lock missing.
  cargoLockPath = src + "/Cargo.lock";
  hasCargoLockHash = (resolved.cargoLockHash or null) != null;
  cargoLockExists = builtins.pathExists cargoLockPath;
  shouldCheck = !skipStalenessCheck && hasCargoLockHash && cargoLockExists;
  currentHash = if shouldCheck then builtins.hashFile "sha256" cargoLockPath else "";
  _stalenessCheck =
    if shouldCheck && currentHash != resolved.cargoLockHash then
      builtins.throw ''
        unit2nix: build-plan.json is out of date!

        The Cargo.lock has changed since build-plan.json was generated.
        Regenerate it with:

          nix run .#update-plan

        Or if you have cargo-unit2nix installed:

          cargo unit2nix -o build-plan.json

        Set skipStalenessCheck = true to bypass this check.

        Expected: ${resolved.cargoLockHash}
        Got:      ${currentHash}
      ''
    else
      true;

  # Cross-compilation target check: warn when the build plan's target triple
  # doesn't match the pkgs host platform. This catches silent mismatches where
  # e.g. an x86_64 plan is used with aarch64 pkgs (or vice versa).
  planTarget = resolved.target or null;
  hostConfig = pkgs.stdenv.hostPlatform.config;
  _targetCheck =
    if planTarget != null && planTarget != hostConfig then
      builtins.trace
        ("unit2nix: WARNING — build plan target '${planTarget}' differs from "
          + "pkgs host platform '${hostConfig}'. "
          + "For cross-compilation, use: pkgs.pkgsCross.<platform> or matching --target.")
        true
    else
      true;

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
          packageId: _: buildCrate self cratePkgs buildRustCrate packageId
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

      # Resolve a normal dependency to its derivation.
      # Proc-macro deps must be built for the build platform (they run at compile time).
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

      # Build dependencies always run on the build platform (they're compiled
      # into the build script which executes at build time, not on the target).
      buildDepDrv = dep: self.build.crates.${dep.packageId};

      dependencies = map depDrv (crateInfo.dependencies or [ ]);
      buildDependencies = map buildDepDrv (crateInfo.buildDependencies or [ ]);

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

      # Pass a field through to buildRustCrate only when it's non-null.
      optionalField = field:
        lib.optionalAttrs ((crateInfo.${field} or null) != null) {
          ${field} = crateInfo.${field};
        };

      features = crateInfo.features or [ ];
    in
    buildRustCrate (
      {
        crateName = crateInfo.crateName;
        version = crateInfo.version;
        edition = crateInfo.edition or "2021";
        src = crateSrc;
        inherit dependencies buildDependencies crateRenames features;
        procMacro = crateInfo.procMacro or false;
        crateBin = crateInfo.crateBin or [ ];
        authors = crateInfo.authors or [ ];

        # Cargo env vars that buildRustCrate doesn't set.
        # These are needed by crates that use env!() or std::env::var() in
        # build scripts or source (e.g., rmcp uses CARGO_CRATE_NAME,
        # nushell's build.rs reads CARGO_CFG_FEATURE).
        CARGO_CRATE_NAME = builtins.replaceStrings [ "-" ] [ "_" ] crateInfo.crateName;
        CARGO_CFG_FEATURE = builtins.concatStringsSep "," features;
      }
      // optionalField "sha256"
      // optionalField "build"
      // optionalField "libPath"
      // optionalField "libName"
      // optionalField "links"
      // lib.optionalAttrs ((crateInfo.libCrateTypes or [ ]) != [ ]) {
        type = crateInfo.libCrateTypes;
      }
      # Package metadata for CARGO_PKG_* env vars in build scripts
      // optionalField "description"
      // optionalField "homepage"
      // optionalField "license"
      // optionalField "repository"
    );

  builtCrates = mkBuiltByPackageIdByPkgs pkgs;

in
assert _stalenessCheck;
assert _targetCheck;
{
  # Workspace members keyed by crate name → { packageId, build }.
  # Uses the explicit workspaceMembers map from the JSON (set by cargo metadata),
  # not a heuristic based on source type.
  workspaceMembers = lib.mapAttrs (
    _name: packageId: {
      inherit packageId;
      build = builtCrates.crates.${packageId};
    }
  ) (resolved.workspaceMembers or { });

  # Convenience accessor for single-crate projects. For multi-root workspaces
  # (e.g., `--package a --package b`), only the first root is exposed here.
  # Use `workspaceMembers` to access all members by name.
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
      _name: packageId: builtCrates.crates.${packageId}
    ) (resolved.workspaceMembers or { });
  };

  inherit resolved;
  inherit builtCrates;
}
