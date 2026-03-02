# Builds the unit2nix package for use in auto-mode benchmarks.
# Wraps with cargo/rustc on PATH so the sandbox derivation works.
{ pkgs, root }:
let
  unit2nix-unwrapped = pkgs.rustPlatform.buildRustPackage {
    pname = "unit2nix";
    version = "0.1.0";
    src = pkgs.lib.cleanSourceWith {
      src = root;
      filter = path: type:
        let baseName = builtins.baseNameOf path;
        in (pkgs.lib.cleanSourceFilter path type)
          && baseName != "target"
          && baseName != "sample_workspace"
          && baseName != "tests"
          && baseName != "openspec"
          && baseName != "benches"
          && baseName != "result";
    };
    cargoLock.lockFile = root + "/Cargo.lock";
  };
in
pkgs.symlinkJoin {
  name = "unit2nix-wrapped";
  paths = [ unit2nix-unwrapped ];
  nativeBuildInputs = [ pkgs.makeWrapper ];
  postBuild = ''
    for bin in $out/bin/unit2nix $out/bin/cargo-unit2nix; do
      [ -f "$bin" ] && wrapProgram "$bin" \
        --prefix PATH : ${pkgs.lib.makeBinPath [ pkgs.cargo pkgs.rustc ]}
    done
  '';
}
