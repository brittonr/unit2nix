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
  # Path to the pre-resolved JSON file (from unit2nix) — required unless resolvedData is provided
  resolvedJson ? null,
  # Pre-resolved attrset (from plugin or other source) — alternative to resolvedJson
  resolvedData ? null,
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
  # Skip the cross-compilation target check (default: false).
  # Set to true when intentionally cross-compiling with a custom
  # buildRustCrateForPkgs that handles the host/target split.
  skipTargetCheck ? false,
  # Extra arguments passed to clippy-driver (e.g. ["-D" "warnings"]).
  # Used by the .clippy output — has no effect on normal builds.
  clippyArgs ? [],
  # Optional: custom Rust toolchain containing rustc + clippy-driver.
  # When set, the .clippy output uses this instead of pkgs.clippy + pkgs.rustc.
  # Required when buildRustCrateForPkgs uses a custom toolchain (e.g. from
  # rust-overlay) since the sysroot and clippy-driver must match the compiler.
  rustToolchain ? null,
  # Optional: path to Rust stdlib source for `-Z build-std` crates.
  # Required when the build plan contains stdlib crates (core, alloc, etc.).
  # Typically: "${rustToolchain}/lib/rustlib/src/rust"
  rustSrcPath ? null,
  # Optional: filter which workspace members are exposed.
  # When set, only listed member names appear in workspaceMembers,
  # allWorkspaceMembers, test, and clippy outputs.
  # All crates remain in the build plan (Nix laziness means unused ones are never built).
  members ? null,
  # Optional: provide sources for out-of-tree path dependencies.
  #
  # Maps absolute filesystem paths (as they appear in build-plan.json) to Nix
  # store paths. Use this for path deps that live outside the workspace and
  # weren't auto-resolved to git sources by the CLI.
  #
  # Example:
  #   externalSources."/home/user/sibling-repo/crates/foo" = sibling-repo + "/crates/foo";
  #
  # Flake inputs are the typical source:
  #   inputs.sibling-repo = { url = "github:user/sibling-repo"; flake = false; };
  #   externalSources."/home/user/sibling-repo/crates/foo" =
  #     "${sibling-repo}/crates/foo";
  externalSources ? {},
}:

let
  # Accept either a file path (resolvedJson) or an already-parsed attrset (resolvedData)
  resolved =
    if resolvedData != null then
      resolvedData
    else if resolvedJson != null then
      builtins.fromJSON (builtins.unsafeDiscardStringContext (builtins.readFile resolvedJson))
    else
      throw "build-from-unit-graph.nix: either resolvedJson or resolvedData must be provided";

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

          unit2nix

        Or equivalently:

          cargo unit2nix

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
    if skipTargetCheck then
      true
    else if planTarget != null && planTarget != hostConfig then
      builtins.trace
        ("unit2nix: WARNING — build plan target '${planTarget}' differs from "
          + "pkgs host platform '${hostConfig}'. "
          + "For cross-compilation, use: pkgs.pkgsCross.<platform> or matching --target."
          + " Set skipTargetCheck = true to suppress this warning.")
        true
    else
      true;

  # Workspace member filtering: validate and filter when `members` is set.
  allWorkspaceMemberNames = builtins.attrNames (resolved.workspaceMembers or {});
  _membersValidation =
    if members != null then
      let
        invalid = builtins.filter (m: !(lib.elem m allWorkspaceMemberNames)) members;
      in
      if invalid != [] then
        builtins.throw (
          "unit2nix: unknown workspace member(s): ${builtins.concatStringsSep ", " invalid}. "
          + "Valid members: ${builtins.concatStringsSep ", " allWorkspaceMemberNames}"
        )
      else
        true
    else
      true;

  filteredWorkspaceMembers =
    assert _membersValidation;
    if members != null then
      lib.filterAttrs (name: _: lib.elem name members) (resolved.workspaceMembers or {})
    else
      (resolved.workspaceMembers or {});

  fetchSource = import ./fetch-source.nix { inherit pkgs src rustSrcPath externalSources; };

  # Stdlib crate detection for build-std support.
  # When a build plan contains stdlib crates (core, alloc, compiler_builtins),
  # they must be built for the TARGET but NOT for the HOST. The host rustc
  # already provides these via its sysroot — passing them as --extern causes
  # "duplicate lang item" errors.
  stdlibPackageIds = lib.filterAttrs
    (_pid: info: (info.source.type or null) == "stdlib")
    resolved.crates;
  isStdlibCrate = packageId: stdlibPackageIds ? ${packageId};
  hasStdlibCrates = stdlibPackageIds != {};

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
  #
  # isHost: when true, this is the host (build-platform) crate set used for
  # build scripts and proc-macros. Stdlib crates are excluded — the host
  # rustc provides core/alloc via its sysroot.
  mkBuiltByPackageIdByPkgs =
    { isHost ? false }:
    cratePkgs:
    let
      buildRustCrate =
        let
          base = buildRustCrateForPkgs cratePkgs;
        in
        base.override { defaultCrateOverrides = mergedOverrides; };

      # On the host path, exclude stdlib crates entirely. They're never
      # needed — the host compiler has them in its sysroot. Including them
      # would cause "duplicate lang item" errors.
      buildableCrates =
        if isHost && hasStdlibCrates then
          lib.filterAttrs (pid: _: !isStdlibCrate pid) resolved.crates
        else
          resolved.crates;

      self = {
        # Each crate keyed by its full package ID
        crates = lib.mapAttrs (
          packageId: _: buildCrate { inherit isHost; } self cratePkgs buildRustCrate packageId {}
        ) buildableCrates;

        # For proc-macro / build-dep host platform builds
        build = mkBuiltByPackageIdByPkgs { isHost = true; } cratePkgs.buildPackages;
      };
    in
    self;

  # Build a single crate derivation.
  #
  # When `includeDevDeps` is true, devDependencies are appended to the
  # dependency list. This is used by the test build path for workspace members.
  #
  # The `hostCtx` parameter carries build-context flags (isHost) from
  # mkBuiltByPackageIdByPkgs. On the host path, stdlib crate deps are
  # filtered out — the host rustc provides core/alloc via its sysroot.
  buildCrate =
    hostCtx:
    self: cratePkgs: buildRustCrate: packageId:
    { includeDevDeps ? false }:
    let
      isHost = hostCtx.isHost or false;
      crateInfo = resolved.crates.${packageId};

      # Skip stdlib deps on the host path — the host rustc sysroot provides them.
      skipStdlibDep = dep: isHost && hasStdlibCrates && isStdlibCrate dep.packageId;

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

      normalDeps = builtins.filter (dep: !skipStdlibDep dep) (crateInfo.dependencies or [ ]);
      devDeps = if includeDevDeps
        then builtins.filter (dep: !skipStdlibDep dep) (crateInfo.devDependencies or [ ])
        else [ ];

      dependencies = map depDrv (normalDeps ++ devDeps);
      buildDependencies = map buildDepDrv (crateInfo.buildDependencies or [ ]);

      # Compute crate renames: when externCrateName differs from the dep's crateName
      allDeps = normalDeps ++ devDeps ++ (crateInfo.buildDependencies or [ ]);
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

      # For cross builds, host and target can have different features.
      # When isHost, prefer hostFeatures (if present) over target features.
      features =
        if isHost then
          crateInfo.hostFeatures or crateInfo.features or [ ]
        else
          crateInfo.features or [ ];

      # Warn about -sys crates with `links` that have no override configured.
      linksValue = crateInfo.links or null;
      hasLinks = linksValue != null;
      hasOverride = mergedOverrides ? ${crateInfo.crateName};
      isKnownNoOverride = hasLinks && crateOverridesLib.isKnownNoOverride crateInfo.crateName linksValue;
      _linksWarning =
        if hasLinks && !hasOverride && !isKnownNoOverride then
          builtins.trace
            ("unit2nix: WARNING — crate '${crateInfo.crateName}' has links=\"${linksValue}\""
              + " but no override found. It may need native libraries.\n"
              + "  Add to your flake.nix:\n"
              + "    extraCrateOverrides = {\n"
              + "      ${crateInfo.crateName} = attrs: {\n"
              + "        nativeBuildInputs = [ pkgs.pkg-config ];\n"
              + "        buildInputs = [ pkgs.<library> ];\n"
              + "      };\n"
              + "    };\n"
              + "  See docs/sys-crate-overrides.md for details.")
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

        # Cargo sets CARGO_ENCODED_RUSTFLAGS to the encoded rustflags (empty by
        # default). Crates like rav1e and av-scenechange unwrap() on this var.
        CARGO_ENCODED_RUSTFLAGS = "";

        # Cargo sets CARGO_CFG_TARGET_FEATURE to comma-separated CPU features
        # (e.g. "fxsr,sse,sse2" on x86_64). Empty is safe — crates that read
        # this typically gate optional SIMD paths behind specific features.
        CARGO_CFG_TARGET_FEATURE = "";
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

  builtCrates = mkBuiltByPackageIdByPkgs {} pkgs;

  # --- Test support (dev dependencies) ---
  #
  # When the build plan includes devDependencies (generated with --include-dev),
  # the .test output builds workspace members with dev deps included. Non-workspace
  # crates reuse the normal build (same store paths).
  hasDevDeps = builtins.any
    (pid: (resolved.crates.${pid}.devDependencies or []) != [])
    (lib.attrValues (resolved.workspaceMembers or {}));

  mkTestBuiltByPkgs =
    { isHost ? false }:
    cratePkgs:
    let
      normalBuilt = mkBuiltByPackageIdByPkgs { inherit isHost; } cratePkgs;

      buildRustCrate =
        let
          base = buildRustCrateForPkgs cratePkgs;
        in
        base.override { defaultCrateOverrides = mergedOverrides; };

      workspaceMemberIds = lib.attrValues (resolved.workspaceMembers or {});

      buildableCrates =
        if isHost && hasStdlibCrates then
          lib.filterAttrs (pid: _: !isStdlibCrate pid) resolved.crates
        else
          resolved.crates;

      self = {
        crates = lib.mapAttrs (
          packageId: _:
          let
            isWorkspaceMember = lib.elem packageId workspaceMemberIds;
            crateInfo = resolved.crates.${packageId};
            hasDevDepsForCrate = (crateInfo.devDependencies or []) != [];
          in
          if isWorkspaceMember && hasDevDepsForCrate then
            # Rebuild workspace members with dev deps included
            buildCrate { inherit isHost; } self cratePkgs buildRustCrate packageId { includeDevDeps = true; }
          else if isWorkspaceMember then
            # Workspace member without dev deps — still rebuild to link against
            # siblings that may have different dep sets
            buildCrate { inherit isHost; } self cratePkgs buildRustCrate packageId {}
          else
            normalBuilt.crates.${packageId}
        ) buildableCrates;
        build = mkTestBuiltByPkgs { isHost = true; } cratePkgs.buildPackages;
      };
    in
    self;

  testCrates = if hasDevDeps then mkTestBuiltByPkgs {} pkgs else builtCrates;

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
      # When a custom toolchain is provided (e.g. rust-overlay nightly),
      # use it for both clippy-driver and the sysroot. The toolchain must
      # include the clippy component.
      clippyDrv = if rustToolchain != null then rustToolchain else pkgs.clippy;
      rustcDrv = if rustToolchain != null then rustToolchain else pkgs.rustc;
      extraArgs = lib.concatMapStringsSep " " lib.escapeShellArg clippyArgs;
    in
    pkgs.runCommand "clippy-as-rustc"
      { nativeBuildInputs = [ pkgs.makeWrapper ]; }
      ''
        mkdir -p $out/bin $out/lib
        # Symlink the compiler's libs (sysroot) so clippy-driver finds them
        ln -s ${rustcDrv}/lib/* $out/lib/

        # Wrap clippy-driver as "rustc" so buildRustCrate runs clippy
        makeWrapper ${clippyDrv}/bin/clippy-driver $out/bin/rustc \
          ${lib.optionalString (clippyArgs != []) ''--add-flags "${extraArgs}"''}
      '';

  # Build workspace members under clippy, reusing normal dependency builds.
  # Non-workspace crates resolve to the exact same Nix store paths — no
  # redundant compilation.
  mkClippyBuiltByPkgs =
    { isHost ? false }:
    cratePkgs:
    let
      normalBuilt = mkBuiltByPackageIdByPkgs { inherit isHost; } cratePkgs;

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

      buildableCrates =
        if isHost && hasStdlibCrates then
          lib.filterAttrs (pid: _: !isStdlibCrate pid) resolved.crates
        else
          resolved.crates;

      self = {
        crates = lib.mapAttrs (
          packageId: _:
          if lib.elem packageId workspaceMemberIds then
            buildCrate { inherit isHost; } self cratePkgs clippyBuildRustCrate packageId {}
          else
            normalBuilt.crates.${packageId}
        ) buildableCrates;
        # Build-platform crates use clippy for workspace members too,
        # so build scripts see the same rlib metadata as the lib phase.
        build = mkClippyBuiltByPkgs { isHost = true; } cratePkgs.buildPackages;
      };
    in
    self;

  clippyCrates = mkClippyBuiltByPkgs {} pkgs;

in
assert _stalenessCheck;
assert _targetCheck;
{
  # Workspace members keyed by crate name → { packageId, build }.
  # Uses the explicit workspaceMembers map from the JSON (set by cargo metadata),
  # not a heuristic based on source type.
  # When `members` is set, only expose selected members.
  # Internal crate graph (builtCrates, testCrates, clippyCrates) still contains all crates —
  # filtering only affects what's exposed in the output attrset.
  workspaceMembers = lib.mapAttrs (
    _name: packageId: {
      inherit packageId;
      build = builtCrates.crates.${packageId};
    }
  ) filteredWorkspaceMembers;

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
    ) filteredWorkspaceMembers;
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
    ) filteredWorkspaceMembers;

    allWorkspaceMembers = pkgs.symlinkJoin {
      name = "all-workspace-members-test";
      paths = lib.mapAttrsToList (
        _name: packageId: testCrates.crates.${packageId}
      ) filteredWorkspaceMembers;
    };

    # Run test binaries for workspace members.
    # Uses .override { buildTests = true; } on the testCrates build (which already
    # includes dev deps). Dependencies stay as normal lib builds (same store paths);
    # only the workspace member itself is recompiled with `--test`.
    check = lib.mapAttrs (
      _name: packageId:
      let
        # Rebuild this one crate with buildTests — deps remain normal .lib builds
        testBinDrv = (testCrates.crates.${packageId}).override { buildTests = true; };
        crateName = resolved.crates.${packageId}.crateName;
      in
      pkgs.runCommand "test-${crateName}" {} ''
        if [ -d "${testBinDrv}/tests" ]; then
          for t in "${testBinDrv}"/tests/*; do
            if [ -x "$t" ]; then
              echo "Running test: $(basename $t)"
              "$t"
            fi
          done
        fi
        touch "$out"
      ''
    ) filteredWorkspaceMembers;
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
    ) filteredWorkspaceMembers;

    allWorkspaceMembers = pkgs.symlinkJoin {
      name = "all-workspace-members-clippy";
      paths = lib.mapAttrsToList (
        _name: packageId: clippyCrates.crates.${packageId}
      ) filteredWorkspaceMembers;
    };
  };

  inherit resolved;
  inherit builtCrates;
}
