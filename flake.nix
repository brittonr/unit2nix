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
            defaultCrateOverrides ? pkgs.defaultCrateOverrides,
          }:
          import ./lib/build-from-unit-graph.nix {
            inherit
              pkgs
              src
              resolvedJson
              buildRustCrateForPkgs
              defaultCrateOverrides
              ;
          };

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
          inherit buildFromUnitGraph;
        };

        # Packages
        packages = {
          sample = sampleWorkspace.allWorkspaceMembers;
          sample-bin = sampleWorkspace.workspaceMembers."sample-bin".build;
        };

        # Checks
        checks = {
          sample-builds = sampleWorkspace.allWorkspaceMembers;
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

        # Dev shell
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Rust
            rustc
            cargo
            clippy
            rustfmt

            # OpenSpec CLI
            openspec

            # Nix tools
            nixfmt-rfc-style
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
