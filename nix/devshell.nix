# Development shell for unit2nix
{ pkgs }:
pkgs.mkShell {
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

    # Nix tools
    nixfmt
  ];

  shellHook = ''
    echo "unit2nix devshell"
    echo "  cargo build           Build unit2nix"
    echo "  cargo test            Run tests"
  '';
}
