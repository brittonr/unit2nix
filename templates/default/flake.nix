{
  description = "Rust project built with unit2nix";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    unit2nix.url = "github:brittonr/unit2nix";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      nixpkgs,
      unit2nix,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        # Build the workspace from the pre-resolved build plan.
        #
        # Generate/update build-plan.json:
        #   unit2nix
        #
        # Regenerate whenever Cargo.lock changes (unit2nix will warn if stale).
        ws = unit2nix.lib.${system}.buildFromUnitGraph {
          inherit pkgs;
          src = ./.;
          resolvedJson = ./build-plan.json;

          # Override -sys crates that need native C libraries.
          # See: https://github.com/brittonr/unit2nix/blob/main/docs/sys-crate-overrides.md
          #
          # pkgs.defaultCrateOverrides already handles many common crates.
          # Add your own below — keys are crate names, values are override functions.
          #
          # defaultCrateOverrides = pkgs.defaultCrateOverrides // {
          #   openssl-sys = attrs: {
          #     nativeBuildInputs = [ pkgs.pkg-config ];
          #     buildInputs = [ pkgs.openssl.dev ];
          #   };
          #   libz-sys = attrs: {
          #     nativeBuildInputs = [ pkgs.pkg-config ];
          #     buildInputs = [ pkgs.zlib ];
          #     LIBZ_SYS_STATIC = "0";
          #   };
          # };
        };
      in
      {
        # Change "my-crate" to your workspace member name
        packages.default = ws.workspaceMembers."my-crate".build;

        # Or build all workspace members:
        # packages.default = ws.allWorkspaceMembers;

        # Or build a subset of workspace members:
        # ws-subset = unit2nix.lib.${system}.buildFromUnitGraph {
        #   inherit pkgs;
        #   src = ./.;
        #   resolvedJson = ./build-plan.json;
        #   members = [ "my-bin" "my-lib" ];
        # };

        # Uncomment to add an override coverage check to `nix flake check`:
        # checks.overrides = pkgs.runCommand "check-overrides" {
        #   nativeBuildInputs = [
        #     unit2nix.packages.${system}.unit2nix
        #     pkgs.jq
        #   ];
        # } ''
        #   unit2nix --check-overrides --json -o ${./build-plan.json} > report.json
        #   missing=$(jq -r '.missing' report.json)
        #   if [ "$missing" -gt 0 ]; then
        #     echo "Missing overrides detected:"
        #     jq -r '.crates[] | select(.status == "unknown") | "  \(.name) (links=\(.links))"' report.json
        #     exit 1
        #   fi
        #   cp report.json $out
        # '';
      }
    );

  # --- Alternative: flake-parts module (least boilerplate) ---
  #
  # Replace this entire file with the following if you use flake-parts:
  #
  # {
  #   inputs = {
  #     nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  #     flake-parts.url = "github:hercules-ci/flake-parts";
  #     unit2nix.url = "github:brittonr/unit2nix";
  #   };
  #
  #   outputs = inputs@{ flake-parts, ... }:
  #     flake-parts.lib.mkFlake { inherit inputs; } {
  #       imports = [ inputs.unit2nix.flakeModules.default ];
  #       systems = [ "x86_64-linux" "aarch64-linux" ];
  #
  #       unit2nix = {
  #         enable = true;
  #         src = ./.;
  #         resolvedJson = ./build-plan.json;
  #         defaultPackage = "my-crate";
  #       };
  #     };
  # }
  #
  # This auto-wires: packages, checks (clippy + tests), devShell, apps.
  # See: https://github.com/brittonr/unit2nix#or-use-the-flake-parts-module-least-boilerplate

  # --- Alternative: nixpkgs overlay ---
  #
  # Use the overlay to get pkgs.unit2nix (no system threading):
  #
  # let
  #   pkgs = import nixpkgs {
  #     system = "x86_64-linux";
  #     overlays = [ unit2nix.overlays.default ];
  #   };
  #   ws = pkgs.unit2nix.buildFromUnitGraph {
  #     src = ./.;
  #     resolvedJson = ./build-plan.json;
  #   };
  # in ws.workspaceMembers."my-crate".build
}
