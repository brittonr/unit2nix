# The unit2nix CLI binary, wrapped with runtime deps (cargo, nix-prefetch-git, etc.)
{
  lib,
  rustPlatform,
  symlinkJoin,
  makeWrapper,
  cargo,
  rustc,
  nix-prefetch-git,
  nix,
  src,
  cargoLockFile,
}:
let
  unwrapped = rustPlatform.buildRustPackage {
    pname = "unit2nix";
    version = "0.1.0";
    src = lib.cleanSourceWith {
      inherit src;
      filter =
        path: type:
        let
          baseName = builtins.baseNameOf path;
        in
        (lib.cleanSourceFilter path type)
        && baseName != "target"
        && baseName != "sample_workspace"
        && baseName != "tests"
        && baseName != "openspec"
        && baseName != "result";
    };
    cargoLock.lockFile = cargoLockFile;
    meta = {
      description = "Per-crate Nix build plans from Cargo's unit graph";
      license = lib.licenses.mit;
      mainProgram = "unit2nix";
    };
  };
in
# Wrap the binary so nix-prefetch-git is available for git dep prefetching
symlinkJoin {
  name = "unit2nix-${unwrapped.version}";
  paths = [ unwrapped ];
  nativeBuildInputs = [ makeWrapper ];
  postBuild = ''
    for bin in $out/bin/unit2nix $out/bin/cargo-unit2nix; do
      wrapProgram "$bin" \
        --suffix PATH : ${
          lib.makeBinPath [
            cargo
            rustc
            nix-prefetch-git
            nix
          ]
        }
    done
  '';
  inherit (unwrapped) meta version;
}
