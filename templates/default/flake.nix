{
  description = "Rust project built with unit2nix";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    unit2nix.url = "github:brittonr/unit2nix";
  };

  outputs =
    {
      nixpkgs,
      unit2nix,
      ...
    }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};

      # Build the workspace from the pre-resolved build plan.
      #
      # Generate build-plan.json with:
      #   nix run github:brittonr/unit2nix -- --manifest-path ./Cargo.toml -o build-plan.json
      #
      # Regenerate whenever Cargo.lock changes (unit2nix will warn if stale).
      ws = unit2nix.lib.${system}.buildFromUnitGraph {
        inherit pkgs;
        src = ./.;
        resolvedJson = ./build-plan.json;

        # Uncomment and add overrides for -sys crates:
        # defaultCrateOverrides = pkgs.defaultCrateOverrides // {
        #   openssl-sys = attrs: {
        #     nativeBuildInputs = [ pkgs.pkg-config ];
        #     buildInputs = [ pkgs.openssl.dev ];
        #   };
        # };
      };
    in
    {
      # Change "my-crate" to your workspace member name
      packages.${system}.default = ws.workspaceMembers."my-crate".build;

      # Or build all workspace members:
      # packages.${system}.default = ws.allWorkspaceMembers;
    };
}
