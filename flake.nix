{
  description = "unit2nix — per-crate Nix build plans from Cargo's unit graph";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
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
              skipStalenessCheck ? false,
              clippyArgs ? [],
              members ? null,
            }:
            import ./lib/build-from-unit-graph.nix {
              inherit
                pkgs
                src
                resolvedJson
                buildRustCrateForPkgs
                defaultCrateOverrides
                extraCrateOverrides
                skipStalenessCheck
                clippyArgs
                members
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

          crateOverridesLib = import ./lib/crate-overrides.nix { inherit pkgs; };

          sampleWorkspace = buildFromUnitGraph {
            inherit pkgs;
            src = ./sample_workspace;
            resolvedJson = ./sample_workspace/build-plan.json;
          };
        in
        {
          lib = {
            inherit buildFromUnitGraph buildFromUnitGraphAuto buildFromUnitGraphPlugin;
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

      lib = pick "lib";
      packages = pick "packages";
      checks = pick "checks";
      devShells = pick "devShells";
    };
}
