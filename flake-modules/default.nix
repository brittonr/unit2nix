# Flake-parts module for unit2nix.
#
# Usage in consumer's flake.nix:
#
#   {
#     inputs = {
#       nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
#       flake-parts.url = "github:hercules-ci/flake-parts";
#       unit2nix.url = "github:brittonr/unit2nix";
#     };
#
#     outputs = inputs@{ flake-parts, ... }:
#       flake-parts.lib.mkFlake { inherit inputs; } {
#         imports = [ inputs.unit2nix.flakeModules.default ];
#         systems = [ "x86_64-linux" "aarch64-linux" ];
#
#         unit2nix = {
#           enable = true;
#           src = ./.;
#           resolvedJson = ./build-plan.json;
#           defaultPackage = "my-bin";
#         };
#       };
#   }

# This file is called with { unit2nixFlake } from flake.nix, returning a
# flake-parts module. The closure captures the unit2nix flake source so the
# module can apply the overlay and reference the CLI without the consumer
# needing to wire anything manually.

{ unit2nixFlake }:

{ lib, config, ... }:

let
  cfg = config.unit2nix;

  overlay = import ../nix/overlay.nix { self = unit2nixFlake; };
in
{
  options.unit2nix = {
    enable = lib.mkEnableOption "unit2nix Rust build integration";

    src = lib.mkOption {
      type = lib.types.path;
      description = "Workspace source root.";
    };

    resolvedJson = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Path to build-plan.json. When null, uses auto mode (IFD).";
    };

    workspaceDir = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Subdirectory within src for projects with external path deps.";
    };

    defaultPackage = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Workspace member name for packages.default. When null, uses allWorkspaceMembers.";
    };

    members = lib.mkOption {
      type = lib.types.nullOr (lib.types.listOf lib.types.str);
      default = null;
      description = "Workspace member filter. When null, all members are included.";
    };

    extraCrateOverrides = lib.mkOption {
      type = lib.types.attrs;
      default = { };
      description = "Project-specific -sys crate overrides merged on top of defaults.";
    };

    checks = {
      clippy = lib.mkEnableOption "clippy check" // {
        default = true;
      };
      tests = lib.mkEnableOption "test check" // {
        default = true;
      };
      overrides = lib.mkEnableOption "override coverage check" // {
        default = false;
      };
    };

    devShell = {
      enable = lib.mkEnableOption "dev shell with unit2nix + cargo + rustc" // {
        default = true;
      };
      extraPackages = lib.mkOption {
        type = lib.types.listOf lib.types.package;
        default = [ ];
        description = "Extra packages to include in the dev shell.";
      };
    };

    rustToolchain = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = null;
      description = "Custom Rust toolchain (e.g. from rust-overlay). Used in auto mode for nightly cargo.";
    };
  };

  config = lib.mkIf cfg.enable {
    perSystem =
      { pkgs, system, ... }:
      let
        # Apply the overlay to get pkgs.unit2nix
        unit2nixPkgs = (overlay pkgs pkgs).unit2nix;

        # Build the workspace — manual mode or auto mode
        ws =
          if cfg.resolvedJson != null then
            unit2nixPkgs.buildFromUnitGraph (
              {
                src = cfg.src;
                resolvedJson = cfg.resolvedJson;
                extraCrateOverrides = cfg.extraCrateOverrides;
              }
              // lib.optionalAttrs (cfg.members != null) { members = cfg.members; }
            )
          else
            unit2nixPkgs.buildFromUnitGraphAuto (
              {
                src = cfg.src;
                extraCrateOverrides = cfg.extraCrateOverrides;
              }
              // lib.optionalAttrs (cfg.workspaceDir != null) { workspaceDir = cfg.workspaceDir; }
              // lib.optionalAttrs (cfg.members != null) { members = cfg.members; }
              // lib.optionalAttrs (cfg.rustToolchain != null) { rustToolchain = cfg.rustToolchain; }
            );

        # All workspace member names from the build result
        memberNames = builtins.attrNames ws.workspaceMembers;
      in
      {
        # packages.default — specific member or all
        packages =
          {
            default =
              if cfg.defaultPackage != null then
                ws.workspaceMembers.${cfg.defaultPackage}.build
              else
                ws.allWorkspaceMembers;
          }
          # packages.<name> — one per workspace member
          // lib.listToAttrs (
            map (name: {
              inherit name;
              value = ws.workspaceMembers.${name}.build;
            }) memberNames
          );

        checks =
          { }
          // lib.optionalAttrs cfg.checks.clippy {
            unit2nix-clippy = ws.clippy.allWorkspaceMembers;
          }
          // lib.optionalAttrs cfg.checks.tests {
            unit2nix-tests = pkgs.runCommand "unit2nix-tests" { } (
              lib.concatMapStrings (
                name:
                let
                  testCheck = ws.test.check.${name};
                in
                ''
                  echo "Running tests for ${name}..."
                  # test.check derivations are already built — just depend on them
                ''
              ) memberNames
              + ''
                ${lib.concatMapStrings (name: "ln -s ${ws.test.check.${name}} /dev/null 2>/dev/null || true\n") memberNames}
                touch $out
              ''
            );
          }
          // lib.optionalAttrs (cfg.checks.overrides && cfg.resolvedJson != null) {
            unit2nix-overrides = pkgs.runCommand "check-overrides" {
              nativeBuildInputs = [
                unit2nixPkgs.cli
                pkgs.jq
              ];
            } ''
              unit2nix --check-overrides --json -o ${cfg.resolvedJson} > report.json
              missing=$(jq -r '.missing' report.json)
              if [ "$missing" -gt 0 ]; then
                echo "Missing overrides detected:"
                jq -r '.crates[] | select(.status == "unknown") | "  \(.name) (links=\(.links))"' report.json
                exit 1
              fi
              cp report.json $out
            '';
          };

        devShells = lib.optionalAttrs cfg.devShell.enable {
          default = pkgs.mkShell {
            nativeBuildInputs =
              [
                unit2nixPkgs.cli
                pkgs.cargo
                pkgs.rustc
                pkgs.rust-analyzer
              ]
              ++ lib.optional (cfg.rustToolchain != null) cfg.rustToolchain
              ++ cfg.devShell.extraPackages;
          };
        };

        apps = lib.optionalAttrs (cfg.resolvedJson != null) {
          update-plan = {
            type = "app";
            program = toString (
              pkgs.writeShellScript "update-plan" ''
                ${unit2nixPkgs.cli}/bin/unit2nix -o build-plan.json "$@"
              ''
            );
          };
        };
      };
  };
}
