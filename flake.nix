{
  description = "unit2nix — per-crate Nix build plans from Cargo's unit graph";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      ...
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;

      perSystem = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};

          # Library: build a workspace from unit2nix JSON
          buildFromUnitGraph =
            {
              pkgs ? nixpkgs.legacyPackages.${system},
              src,
              resolvedJson,
              buildRustCrateForPkgs ? pkgs: pkgs.buildRustCrate,
              defaultCrateOverrides ? null,
              extraCrateOverrides ? {},
              externalSources ? {},
              skipStalenessCheck ? false,
              clippyArgs ? [],
              members ? null,
              rustToolchain ? null,
            }:
            import ./lib/build-from-unit-graph.nix {
              inherit
                pkgs
                src
                resolvedJson
                buildRustCrateForPkgs
                defaultCrateOverrides
                extraCrateOverrides
                externalSources
                skipStalenessCheck
                clippyArgs
                members
                rustToolchain
                ;
            };

          # Auto mode: generate build plan via IFD (no manual step needed).
          # Requires IFD enabled (default in Nix; disabled on Hydra).
          buildFromUnitGraphAuto =
            {
              pkgs ? nixpkgs.legacyPackages.${system},
              src,
              workspaceDir ? null,
              buildRustCrateForPkgs ? pkgs: pkgs.buildRustCrate,
              defaultCrateOverrides ? null,
              extraCrateOverrides ? {},
              clippyArgs ? [],
              members ? null,
              # Optional: Rust toolchain for the IFD step (e.g. nightly from rust-overlay).
              # `cargo --unit-graph` requires nightly. When set, this toolchain is
              # prepended to PATH, overriding the stable cargo bundled in the unit2nix wrapper.
              rustToolchain ? null,
              # Pass --workspace to cargo for per-crate test support.
              # When true, ALL workspace members and their dev-deps are resolved.
              workspace ? false,
              # Optional: build a specific package (-p flag)
              package ? null,
              # Optional: features to enable (comma-separated string)
              features ? null,
              # Optional: enable all features
              allFeatures ? false,
              # Optional: disable default features
              noDefaultFeatures ? false,
              # Optional: include dev-dependencies
              includeDev ? false,
              # Optional: build a specific binary target (--bin flag).
              # More restrictive than package: only captures deps for one binary.
              bin ? null,
              # Don't pass --locked to cargo.
              noLocked ? false,
              # Sources for out-of-tree path dependencies.
              # Maps relative paths (as in Cargo.toml) to Nix store paths.
              # Example: externalSources = { "../sibling" = sibling-input; };
              externalSources ? {},
            }:
            import ./lib/auto.nix {
              inherit
                pkgs
                src
                workspaceDir
                buildRustCrateForPkgs
                defaultCrateOverrides
                extraCrateOverrides
                clippyArgs
                members
                rustToolchain
                workspace
                package
                bin
                noLocked
                features
                allFeatures
                noDefaultFeatures
                includeDev
                externalSources
                ;
              unit2nix = self.packages.${system}.unit2nix;
            };

          # Plugin-based builder (requires plugin loaded)
          buildFromUnitGraphPlugin =
            {
              pkgs ? nixpkgs.legacyPackages.${system},
              src,
              buildRustCrateForPkgs ? pkgs: pkgs.buildRustCrate,
              defaultCrateOverrides ? null,
              extraCrateOverrides ? {},
              clippyArgs ? [],
              members ? null,
              target ? null,
              includeDev ? false,
              features ? null,
              allFeatures ? false,
              noDefaultFeatures ? false,
              bin ? null,
              package ? null,
            }:
            import ./lib/plugin.nix {
              inherit
                pkgs
                src
                buildRustCrateForPkgs
                defaultCrateOverrides
                extraCrateOverrides
                clippyArgs
                members
                target
                includeDev
                features
                allFeatures
                noDefaultFeatures
                bin
                package
                ;
            };

          unit2nix = pkgs.callPackage ./nix/package.nix {
            src = ./.;
            cargoLockFile = ./Cargo.lock;
          };

          unit2nixPlugin = pkgs.callPackage ./nix/plugin.nix {
            nixComponents = pkgs.nixVersions.nixComponents_2_33;
          };

          # Vendor crate sources from a single Cargo.lock.
          # Returns { vendoredSources, cargoConfig, gitCheckouts }.
          vendorCargoDeps =
            {
              pkgs ? nixpkgs.legacyPackages.${system},
              cargoLock,
              crateHashesJson ? null,
            }:
            import ./lib/vendor.nix {
              inherit pkgs cargoLock crateHashesJson;
            };

          # Vendor crate sources from multiple Cargo.lock files.
          # Merges all lock files and produces a single vendor directory.
          # Returns { vendoredSources, cargoConfig, gitCheckouts }.
          vendorMultipleCargoDeps =
            {
              pkgs ? nixpkgs.legacyPackages.${system},
              cargoLocks,
              crateHashesJson ? null,
            }:
            let
              lib = pkgs.lib;

              # Parse all lock files
              allLocked = map (lock: lib.importTOML lock) cargoLocks;

              # Merge all packages, deduplicating by (name, version, source)
              allPackages = builtins.concatLists (map (l: l.package or [ ]) allLocked);
              withSource = builtins.filter (p: p ? source) allPackages;
              byId = builtins.listToAttrs (
                map (p: { name = "${p.name} ${p.version} (${p.source})"; value = p; }) withSource
              );

              # Write a synthetic merged lock file for vendor.nix
              mergedLock = pkgs.writeText "merged-Cargo.lock" (
                builtins.toJSON {
                  package = builtins.attrValues byId;
                }
              );

              # vendor.nix expects TOML, but we can just pass the merged packages
              # directly. Create a minimal TOML lock file.
              mergedLockToml = pkgs.writeText "merged-Cargo.lock" (
                "# Merged lock file\nversion = 3\n\n"
                + lib.concatMapStrings (p:
                  "[[package]]\n"
                  + "name = ${builtins.toJSON p.name}\n"
                  + "version = ${builtins.toJSON p.version}\n"
                  + lib.optionalString (p ? source) "source = ${builtins.toJSON p.source}\n"
                  + lib.optionalString (p ? checksum) "checksum = ${builtins.toJSON p.checksum}\n"
                  + "\n"
                ) (builtins.attrValues byId)
              );
            in
            import ./lib/vendor.nix {
              inherit pkgs crateHashesJson;
              cargoLock = mergedLockToml;
            };

          crateOverridesLib = import ./lib/crate-overrides.nix { inherit pkgs; };

          sampleWorkspace = buildFromUnitGraph {
            inherit pkgs;
            src = ./sample_workspace;
            resolvedJson = ./sample_workspace/build-plan.json;
          };
        in
        {
          lib = {
            inherit
              buildFromUnitGraph
              buildFromUnitGraphAuto
              buildFromUnitGraphPlugin
              vendorCargoDeps
              vendorMultipleCargoDeps
              ;
            crateOverrides = crateOverridesLib.overrides;
            isKnownNoOverride = crateOverridesLib.isKnownNoOverride;
          };

          packages = {
            default = unit2nix;
            inherit unit2nix unit2nixPlugin;
            sample = sampleWorkspace.allWorkspaceMembers;
            sample-bin = sampleWorkspace.workspaceMembers."sample-bin".build;
          };

          checks = import ./nix/checks.nix {
            inherit
              pkgs
              self
              system
              buildFromUnitGraph
              buildFromUnitGraphAuto
              unit2nix
              sampleWorkspace
              ;
          };

          devShells.default = import ./nix/devshell.nix { inherit pkgs; };
        }
      );

      pick = attr: nixpkgs.lib.mapAttrs (_: v: v.${attr}) perSystem;
    in
    {
      templates.default = {
        description = "Rust project with unit2nix per-crate Nix builds";
        path = ./templates/default;
      };

      overlays.default = import ./nix/overlay.nix { inherit self; };

      flakeModules.default = import ./flake-modules/default.nix { unit2nixFlake = self; };

      lib = pick "lib";
      packages = pick "packages";
      checks = pick "checks";
      devShells = pick "devShells";
    };
}
