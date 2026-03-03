{
  lib,
  stdenv,
  nixComponents,
  rustPlatform,
  cargo,
  rustc,
  pkg-config,
  cmake,
  boost,
  nlohmann_json,
}:

let
  # Build the Rust static library with baked-in cargo + rustc paths
  rustLib = rustPlatform.buildRustPackage {
    pname = "unit2nix-plugin-core";
    version = "0.1.0";
    src = lib.cleanSourceWith {
      src = ./..;
      filter = path: type:
        let baseName = builtins.baseNameOf path; in
        (lib.cleanSourceFilter path type)
        && baseName != "target"
        && baseName != "sample_workspace"
        && baseName != "tests"
        && baseName != "openspec"
        && baseName != "result"
        && baseName != "plugin";
    };
    cargoLock.lockFile = ../Cargo.lock;
    
    # Bake in cargo + rustc paths so the plugin can shell out at eval time
    UNIT2NIX_CARGO_PATH = lib.getExe cargo;
    UNIT2NIX_RUSTC_PATH = lib.getExe rustc;
    
    # Only build the static library, not the binaries
    buildPhase = ''
      runHook preBuild
      cargo build --release --lib
      runHook postBuild
    '';
    
    installPhase = ''
      runHook preInstall
      mkdir -p $out/lib
      cp target/release/libunit2nix.a $out/lib/
      runHook postInstall
    '';
  };
in
stdenv.mkDerivation {
  pname = "unit2nix-plugin";
  version = "0.1.0";

  src = ../plugin;

  nativeBuildInputs = [
    pkg-config
    cmake
  ];

  buildInputs = [
    nixComponents.nix-expr
    boost
    nlohmann_json
  ];

  cmakeFlags = [
    "-DRUST_LIB_DIR=${rustLib}/lib"
  ];

  meta = {
    description = "Nix plugin for unit2nix — resolves Cargo workspaces via unit-graph";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux ++ lib.platforms.darwin;
  };
}
