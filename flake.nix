{
  description = "unit2nix — per-crate Nix build plans from Cargo's unit graph";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      ...
    }:
    {
      # Flake template (not per-system)
      templates.default = {
        description = "Rust project with unit2nix per-crate Nix builds";
        path = ./templates/default;
      };
    }
    //
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        openspec = pkgs.writeShellScriptBin "openspec" ''
          export PATH="${pkgs.nodejs_22}/bin:$PATH"
          exec npx -y @fission-ai/openspec@latest "$@"
        '';

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
              ;
            unit2nix = self.packages.${system}.unit2nix;
          };

        # The unit2nix binary itself
        unit2nix =
          let
            unwrapped = pkgs.rustPlatform.buildRustPackage {
              pname = "unit2nix";
              version = "0.1.0";
              src = pkgs.lib.cleanSourceWith {
                src = ./.;
                filter =
                  path: type:
                  let
                    baseName = builtins.baseNameOf path;
                  in
                  (pkgs.lib.cleanSourceFilter path type)
                  && baseName != "target"
                  && baseName != "sample_workspace"
                  && baseName != "tests"
                  && baseName != "openspec"
                  && baseName != "result";
              };
              cargoLock.lockFile = ./Cargo.lock;
              meta = {
                description = "Per-crate Nix build plans from Cargo's unit graph";
                license = pkgs.lib.licenses.mit;
                mainProgram = "unit2nix";
              };
            };
          in
          # Wrap the binary so nix-prefetch-git is available for git dep prefetching
          pkgs.symlinkJoin {
            name = "unit2nix-${unwrapped.version}";
            paths = [ unwrapped ];
            nativeBuildInputs = [ pkgs.makeWrapper ];
            postBuild = ''
              for bin in $out/bin/unit2nix $out/bin/cargo-unit2nix; do
                wrapProgram "$bin" \
                  --prefix PATH : ${pkgs.lib.makeBinPath [
                    pkgs.cargo
                    pkgs.rustc
                  ]} \
                  --suffix PATH : ${pkgs.lib.makeBinPath [
                    pkgs.nix-prefetch-git
                    pkgs.nix
                  ]}
              done
            '';
            inherit (unwrapped) meta version;
          };

        # Built-in crate overrides registry
        crateOverridesLib = import ./lib/crate-overrides.nix { inherit pkgs; };

        # Sample workspace build
        sampleWorkspace = buildFromUnitGraph {
          inherit pkgs;
          src = ./sample_workspace;
          resolvedJson = ./sample_workspace/build-plan.json;
        };
      in
      {
        # Library output
        lib = {
          inherit buildFromUnitGraph buildFromUnitGraphAuto;
          # Built-in override registry — users can inspect/extend
          crateOverrides = crateOverridesLib.overrides;
          knownNoOverride = crateOverridesLib.knownNoOverride;
          isKnownNoOverride = crateOverridesLib.isKnownNoOverride;
        };

        # Packages
        packages = {
          default = unit2nix;
          inherit unit2nix;
          sample = sampleWorkspace.allWorkspaceMembers;
          sample-bin = sampleWorkspace.workspaceMembers."sample-bin".build;
        };

        # Checks
        checks = {
          sample-builds = sampleWorkspace.allWorkspaceMembers;
          sample-clippy = sampleWorkspace.clippy.allWorkspaceMembers;
          sample-test-deps = sampleWorkspace.test.allWorkspaceMembers;
          sample-run-tests = sampleWorkspace.test.check."sample-lib";
          sample-run-tests-bin = sampleWorkspace.test.check."sample-bin";

          # Members filter: build only sample-bin from 4-member workspace
          sample-members-filter = (buildFromUnitGraph {
            inherit pkgs;
            src = ./sample_workspace;
            resolvedJson = ./sample_workspace/build-plan.json;
            members = [ "sample-bin" ];
          }).allWorkspaceMembers;

          # Auto mode (IFD): builds sample_workspace with no pre-generated JSON
          sample-auto = (buildFromUnitGraphAuto {
            inherit pkgs;
            src = ./sample_workspace;
          }).allWorkspaceMembers;

          # Override coverage check: verify no unknown -sys crates in bat's plan
          check-overrides-bat = pkgs.runCommand "check-overrides-bat" {
            nativeBuildInputs = [ unit2nix pkgs.jq ];
          } ''
            unit2nix --check-overrides --json -o ${./tests/bat/build-plan.json} > report.json
            missing=$(jq -r '.missing' report.json)
            if [ "$missing" -gt 0 ]; then
              echo "Missing overrides detected:"
              jq -r '.crates[] | select(.status == "unknown") | "  \(.name) (links=\(.links))"' report.json
              exit 1
            fi
            cp report.json $out
          '';

          # Real-world validation: pure Rust workspace (34 crates)
          validate-ripgrep = import ./tests/ripgrep/build.nix { inherit pkgs; };

          # Real-world validation: workspace with -sys crates (168 crates)
          validate-bat = import ./tests/bat/build.nix { inherit pkgs; };

          # Real-world validation: pure Rust file finder (59 crates, jemalloc)
          validate-fd = import ./tests/fd/build.nix { inherit pkgs; };

          # Real-world validation: largest test — 519 crates, 29 workspace members
          validate-nushell = import ./tests/nushell/build.nix { inherit pkgs; };
        } // pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
          # VM integration tests (Linux only — requires QEMU/KVM)
          vm-sample-bin = import ./tests/vm/sample-bin.nix {
            inherit pkgs;
            sampleBin = self.packages.${system}.sample-bin;
          };
          vm-per-crate-caching = import ./tests/vm/per-crate-caching.nix {
            inherit pkgs sampleWorkspace;
          };
          vm-rebuild-isolation = import ./tests/vm/rebuild-isolation.nix {
            inherit pkgs sampleWorkspace;
          };
        };

        # Apps
        apps.update-plan = {
          type = "app";
          program = toString (pkgs.writeShellScript "update-plan" ''
            exec ${unit2nix}/bin/unit2nix \
              --manifest-path ./Cargo.toml \
              -o build-plan.json
          '');
        };

        # Dev shell
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Rust
            rustc
            cargo
            clippy
            rustfmt

            # Git dep prefetching
            nix-prefetch-git

            # Benchmarking
            hyperfine
            crate2nix

            # OpenSpec CLI
            openspec

            # Nix tools
            nixfmt
          ];

          shellHook = ''
            echo "unit2nix devshell"
            echo "  openspec --version    OpenSpec CLI"
            echo "  cargo build           Build unit2nix"
            echo "  cargo test            Run tests"
          '';
        };
      }
    );
}
