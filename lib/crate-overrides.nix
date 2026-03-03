# unit2nix built-in crate overrides.
#
# These supplement nixpkgs' defaultCrateOverrides with overrides for common
# -sys crates that nixpkgs doesn't cover (or covers inadequately for
# buildRustCrate). The merge order is:
#
#   pkgs.defaultCrateOverrides  →  unit2nix overrides  →  user extraCrateOverrides
#
# Each entry is a function: attrs -> { nativeBuildInputs, buildInputs, ... }
# matching the buildRustCrate override convention.

{ pkgs }:

let
  inherit (pkgs) lib stdenv;
  isDarwin = stdenv.hostPlatform.isDarwin;

  # Crates whose `links` field is Rust-internal and never needs native library
  # overrides. Used to suppress eval-time "missing override" warnings.
  knownNoOverrideSet = {
    "rayon-core" = true;
    "prettyplease" = true;
    "compiler_builtins" = true;
    "rustc-std-workspace-core" = true;
    "rustc-std-workspace-alloc" = true;
  };

  # Prefix patterns for links values that are Rust-internal.
  knownNoOverridePrefixes = [
    "ring_core_"      # ring's internal links values (e.g., ring_core_0_17_14_)
  ];
in
{
  # Set of crate names that have `links` but need no native override.
  # Exported so build-from-unit-graph.nix can suppress warnings for these.
  knownNoOverride = knownNoOverrideSet;

  # Check if a crate is known to need no native override.
  # Checks both exact crate name match and links value prefix patterns.
  isKnownNoOverride = crateName: linksValue:
    knownNoOverrideSet ? ${crateName}
    || builtins.any (prefix: lib.hasPrefix prefix linksValue) knownNoOverridePrefixes;

  # The override attrset, suitable for merging with defaultCrateOverrides.
  overrides = {
    # --- Crates NOT in nixpkgs defaultCrateOverrides ---

    # ring: crypto library with assembly routines compiled via cc.
    # Needs perl for its build script. Do NOT set RING_PREGENERATE_ASM —
    # that tries to regenerate asm and fails when pregenerated/ dir exists.
    ring = attrs: {
      nativeBuildInputs = [ pkgs.perl ];
      buildInputs = lib.optionals isDarwin [
        pkgs.darwin.apple_sdk.frameworks.Security
      ];
    };

    # tikv-jemalloc-sys: vendors and builds jemalloc from source.
    # Needs make for the vendored C build.
    tikv-jemalloc-sys = attrs: {
      nativeBuildInputs = [ pkgs.gnumake ];
      buildInputs = lib.optionals isDarwin [
        pkgs.darwin.apple_sdk.frameworks.Security
      ];
    };

    # onig_sys: Oniguruma regex library bindings.
    onig_sys = attrs: {
      nativeBuildInputs = [ pkgs.pkg-config ];
      buildInputs = [ pkgs.oniguruma ];
    };

    # librocksdb-sys: RocksDB bindings. Can vendor or use system lib.
    librocksdb-sys = attrs: {
      nativeBuildInputs = [ pkgs.cmake ];
      buildInputs = [ pkgs.rocksdb ];
      ROCKSDB_LIB_DIR = "${pkgs.rocksdb}/lib";
    };

    # zstd-sys: Zstandard compression. Common in CLI tools.
    zstd-sys = attrs: {
      nativeBuildInputs = [ pkgs.pkg-config ];
      buildInputs = [ pkgs.zstd ];
    };

    # bzip2-sys: bzip2 compression.
    bzip2-sys = attrs: {
      nativeBuildInputs = [ pkgs.pkg-config ];
      buildInputs = [ pkgs.bzip2 ];
    };

    # lzma-sys: LZMA/XZ compression.
    lzma-sys = attrs: {
      nativeBuildInputs = [ pkgs.pkg-config ];
      buildInputs = [ pkgs.xz ];
    };

    # jemalloc-sys: jemalloc allocator (non-tikv variant).
    jemalloc-sys = attrs: {
      nativeBuildInputs = [ pkgs.gnumake ];
      buildInputs = lib.optionals isDarwin [
        pkgs.darwin.apple_sdk.frameworks.Security
      ];
    };
  };
}
