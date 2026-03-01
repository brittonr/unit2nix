{
  description = "unit2nix — per-crate Nix build plans from Cargo's unit graph";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      let
        openspec = pkgs.writeShellScriptBin "openspec" ''
          export PATH="${pkgs.nodejs_22}/bin:$PATH"
          exec npx -y @fission-ai/openspec@latest "$@"
        '';
      in
      {
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
