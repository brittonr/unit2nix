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
  # Optional: full override of the crate overrides base layer.
  # When provided, replaces BOTH pkgs.defaultCrateOverrides AND unit2nix built-ins.
  # For most users, use extraCrateOverrides instead.
  defaultCrateOverrides ? null,
  # Optional: additional crate overrides merged ON TOP of the default stack
  # (pkgs.defaultCrateOverrides + unit2nix built-ins). Use this for project-specific
  # -sys crate overrides without repeating well-known boilerplate.
  extraCrateOverrides ? {},
  # Skip the Cargo.lock staleness check (default: false).
  # Set to true when src filtering strips Cargo.lock or for other edge cases.
  skipStalenessCheck ? false,
  # Extra arguments passed to clippy-driver (e.g. ["-D" "warnings"]).
  # Used by the .clippy output — has no effect on normal builds.
  clippyArgs ? [],
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

  # Built-in crate overrides from unit2nix (ring, tikv-jemalloc-sys, etc.)
  crateOverridesLib = import ./crate-overrides.nix { inherit pkgs; };

  # Three-layer override merge:
  #   1. pkgs.defaultCrateOverrides (nixpkgs community overrides)
  #   2. unit2nix built-in overrides (crate-overrides.nix)
  #   3. user extraCrateOverrides (project-specific)
  #
  # When defaultCrateOverrides is explicitly passed, it replaces layers 1+2.
  mergedOverrides =
    if defaultCrateOverrides != null then
      # User took full control — use their base, merge extra on top
      defaultCrateOverrides // extraCrateOverrides
    else
      # Default: nixpkgs → unit2nix built-ins → user extras
      pkgs.defaultCrateOverrides // crateOverridesLib.overrides // extraCrateOverrides;

  # Build the recursive crate set for a given pkgs instance.
  mkBuiltByPackageIdByPkgs =
    cratePkgs:
    let
      buildRustCrate =
        let
          base = buildRustCrateForPkgs cratePkgs;
        in
        base.override { defaultCrateOverrides = mergedOverrides; };

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

      # Warn about -sys crates with `links` that have no override configured.
      linksValue = crateInfo.links or null;
      hasLinks = linksValue != null;
      hasOverride = mergedOverrides ? ${crateInfo.crateName};
      isKnownNoOverride = hasLinks && crateOverridesLib.isKnownNoOverride crateInfo.crateName linksValue;
      _linksWarning =
        if hasLinks && !hasOverride && !isKnownNoOverride then
          builtins.trace
            ("unit2nix: WARNING — crate '${crateInfo.crateName}' has links=\"${linksValue}\""
              + " but no override found. It may need native libraries."
              + " See docs/sys-crate-overrides.md or use extraCrateOverrides.")
            true
        else
          true;
    in
    assert _linksWarning;
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

  # --- Test support (dev dependencies) ---
  #
  # When the build plan includes devDependencies (generated with --include-dev),
  # the .test output builds workspace members with dev deps included. Non-workspace
  # crates reuse the normal build (same store paths).
  hasDevDeps = builtins.any
    (pid: (resolved.crates.${pid}.devDependencies or []) != [])
    (lib.attrValues (resolved.workspaceMembers or {}));

  mkTestBuiltByPkgs =
    cratePkgs:
    let
      normalBuilt = mkBuiltByPackageIdByPkgs cratePkgs;

      buildRustCrate =
        let
          base = buildRustCrateForPkgs cratePkgs;
        in
        base.override { defaultCrateOverrides = mergedOverrides; };

      workspaceMemberIds = lib.attrValues (resolved.workspaceMembers or {});

      self = {
        crates = lib.mapAttrs (
          packageId: _:
          let
            isWorkspaceMember = lib.elem packageId workspaceMemberIds;
            crateInfo = resolved.crates.${packageId};
            hasDevDepsForCrate = (crateInfo.devDependencies or []) != [];
          in
          if isWorkspaceMember && hasDevDepsForCrate then
            # Rebuild workspace members with dev deps added to dependencies
            buildCrateWithDevDeps self cratePkgs buildRustCrate packageId
          else if isWorkspaceMember then
            # Workspace member without dev deps — still rebuild to link against
            # siblings that may have different dep sets
            buildCrate self cratePkgs buildRustCrate packageId
          else
            normalBuilt.crates.${packageId}
        ) resolved.crates;
        build = mkTestBuiltByPkgs cratePkgs.buildPackages;
      };
    in
    self;

  # Build a crate with dev dependencies included (for workspace members only).
  buildCrateWithDevDeps =
    self: cratePkgs: buildRustCrate: packageId:
    let
      crateInfo = resolved.crates.${packageId};

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

      buildDepDrv = dep: self.build.crates.${dep.packageId};

      # Include both regular and dev dependencies
      dependencies = map depDrv (
        (crateInfo.dependencies or [ ]) ++ (crateInfo.devDependencies or [ ])
      );
      buildDependencies = map buildDepDrv (crateInfo.buildDependencies or [ ]);

      allDeps = (crateInfo.dependencies or [ ])
        ++ (crateInfo.devDependencies or [ ])
        ++ (crateInfo.buildDependencies or [ ]);
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
      // optionalField "description"
      // optionalField "homepage"
      // optionalField "license"
      // optionalField "repository"
    );

  testCrates = if hasDevDeps then mkTestBuiltByPkgs pkgs else builtCrates;

  # --- Clippy support ---
  #
  # clippy-driver is a drop-in replacement for rustc — same CLI flags, same
  # output artifacts, but also runs lint passes. We build a wrapper package
  # that exposes bin/rustc → clippy-driver so buildRustCrate (which invokes
  # `noisily rustc …`) runs clippy instead.
  #
  # Dependencies are built with the real rustc (and cached); only workspace
  # members use the clippy wrapper. All workspace members consistently use
  # clippy-driver so inter-member rlib metadata matches.

  clippyRustcWrapper =
    let
      clippy = pkgs.clippy;
      rustc = pkgs.rustc;
      extraArgs = lib.concatMapStringsSep " " lib.escapeShellArg clippyArgs;
    in
    pkgs.runCommand "clippy-as-rustc"
      { nativeBuildInputs = [ pkgs.makeWrapper ]; }
      ''
        mkdir -p $out/bin $out/lib
        # Symlink the real rustc's libs (sysroot) so clippy-driver finds them
        ln -s ${rustc}/lib/* $out/lib/

        # Wrap clippy-driver as "rustc" so buildRustCrate runs clippy
        makeWrapper ${clippy}/bin/clippy-driver $out/bin/rustc \
          ${lib.optionalString (clippyArgs != []) ''--add-flags "${extraArgs}"''}

        # Forward other tools from the real toolchain
        for tool in rustdoc rustfmt; do
          if [ -e ${rustc}/bin/$tool ]; then
            ln -s ${rustc}/bin/$tool $out/bin/$tool
          fi
        done
      '';

  # Build workspace members under clippy, reusing normal dependency builds.
  # Non-workspace crates resolve to the exact same Nix store paths — no
  # redundant compilation.
  mkClippyBuiltByPkgs =
    cratePkgs:
    let
      normalBuilt = mkBuiltByPackageIdByPkgs cratePkgs;

      # Normal buildRustCrate for dependencies (fully cached)
      normalBuildRustCrate =
        let
          base = buildRustCrateForPkgs cratePkgs;
        in
        base.override { defaultCrateOverrides = mergedOverrides; };

      # Clippy buildRustCrate: use clippy-driver as the compiler
      clippyBuildRustCrate = args:
        (normalBuildRustCrate args).override { rust = clippyRustcWrapper; };

      workspaceMemberIds = lib.attrValues (resolved.workspaceMembers or {});

      self = {
        crates = lib.mapAttrs (
          packageId: _:
          if lib.elem packageId workspaceMemberIds then
            buildCrate self cratePkgs clippyBuildRustCrate packageId
          else
            normalBuilt.crates.${packageId}
        ) resolved.crates;
        # Build-platform crates use clippy for workspace members too,
        # so build scripts see the same rlib metadata as the lib phase.
        build = mkClippyBuiltByPkgs cratePkgs.buildPackages;
      };
    in
    self;

  clippyCrates = mkClippyBuiltByPkgs pkgs;

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

  # Test: workspace members built with dev-dependencies included.
  # Only available when the build plan was generated with --include-dev.
  # Non-workspace dependencies are reused from the normal build (same store paths).
  test = {
    workspaceMembers = lib.mapAttrs (
      _name: packageId: {
        inherit packageId;
        build = testCrates.crates.${packageId};
      }
    ) (resolved.workspaceMembers or {});

    allWorkspaceMembers = pkgs.symlinkJoin {
      name = "all-workspace-members-test";
      paths = lib.mapAttrsToList (
        _name: packageId: testCrates.crates.${packageId}
      ) (resolved.workspaceMembers or {});
    };
  };

  # Clippy: workspace members checked with clippy-driver, dependencies
  # compiled normally (cached). Build any member to get clippy diagnostics;
  # the build fails if clippy reports errors.
  clippy = {
    workspaceMembers = lib.mapAttrs (
      _name: packageId: {
        inherit packageId;
        build = clippyCrates.crates.${packageId};
      }
    ) (resolved.workspaceMembers or {});

    allWorkspaceMembers = pkgs.symlinkJoin {
      name = "all-workspace-members-clippy";
      paths = lib.mapAttrsToList (
        _name: packageId: clippyCrates.crates.${packageId}
      ) (resolved.workspaceMembers or {});
    };
  };

  inherit resolved;
  inherit builtCrates;
}
