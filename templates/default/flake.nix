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
        # Generate/update build-plan.json with:
        #   nix run .#update-plan
        #
        # Regenerate whenever Cargo.lock changes (unit2nix will warn if stale).
        # You can also use: cargo unit2nix -o build-plan.json (after cargo install cargo-unit2nix)
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

        # Regenerate build-plan.json when Cargo.lock changes.
        # After generation, unit2nix automatically prints an override coverage
        # summary showing which -sys crates need native library overrides.
        apps.update-plan = {
          type = "app";
          program = toString (pkgs.writeShellScript "update-plan" ''
            exec ${unit2nix.packages.${system}.unit2nix}/bin/unit2nix \
              --manifest-path ./Cargo.toml \
              -o build-plan.json
          '');
        };

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
}
