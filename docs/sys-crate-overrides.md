# Overriding -sys crates

Most common -sys crates work out of the box. unit2nix ships built-in overrides for popular crates like `ring`, `tikv-jemalloc-sys`, and `onig_sys`, and inherits nixpkgs' overrides for `openssl-sys`, `libgit2-sys`, `libz-sys`, and [~60 others](https://github.com/NixOS/nixpkgs/blob/master/pkgs/build-support/rust/default-crate-overrides.nix).

## Quick check

Before building, check if your project needs any overrides:

```bash
unit2nix --check-overrides -o build-plan.json
```

Example output:
```
Found 5 crate(s) with native link requirements:

  ✓ libgit2-sys                     links=git2                  (covered — needs pkg-config + libgit2)
  ✓ libz-sys                        links=z                     (covered — needs pkg-config + zlib)
  ✓ onig_sys                        links=onig                  (covered — needs pkg-config + oniguruma)
  ✓ prettyplease                    links=prettyplease02        (no override needed — Rust-internal)
  ✓ rayon-core                      links=rayon-core            (no override needed — Rust-internal)

Summary: 3 covered, 2 no-override-needed, 0 may need attention
```

If everything is covered, you're done — no overrides needed.

## Override hierarchy

unit2nix merges overrides in three layers:

| Layer | Source | What it covers |
|-------|--------|---------------|
| 1 | `pkgs.defaultCrateOverrides` | ~60 crates from nixpkgs (openssl-sys, libgit2-sys, etc.) |
| 2 | unit2nix built-ins | ring, tikv-jemalloc-sys, onig_sys, zstd-sys, etc. |
| 3 | `extraCrateOverrides` | Your project-specific overrides |

Later layers override earlier ones. You only need to write overrides for crates not covered by layers 1 and 2.

## Adding project-specific overrides

Use `extraCrateOverrides` for crates specific to your project:

```nix
buildFromUnitGraph {
  inherit pkgs src;
  resolvedJson = ./build-plan.json;
  extraCrateOverrides = {
    my-custom-sys = attrs: {
      nativeBuildInputs = [ pkgs.pkg-config ];
      buildInputs = [ pkgs.some-library ];
    };
  };
};
```

The three fields you'll use most:

| Field | Purpose | Example |
|-------|---------|---------|
| `nativeBuildInputs` | Tools needed at build time (runs on host) | `pkg-config`, `cmake`, `protobuf` |
| `buildInputs` | Libraries needed at build + link time | `openssl.dev`, `zlib`, `libgit2` |
| Environment variables | Control build script behavior | `LIBGIT2_NO_VENDOR = "1"` |

## Full control with defaultCrateOverrides

If you need to replace the entire override stack (bypass both nixpkgs and unit2nix built-ins):

```nix
buildFromUnitGraph {
  inherit pkgs src;
  resolvedJson = ./build-plan.json;
  # Replaces ALL default overrides — you're fully in control
  defaultCrateOverrides = {
    my-sys = attrs: { buildInputs = [ pkgs.mylib ]; };
  };
};
```

> **Note:** When `defaultCrateOverrides` is provided, nixpkgs defaults and unit2nix built-ins are NOT applied. Use `extraCrateOverrides` instead unless you need full control.

## Built-in overrides reference

unit2nix ships overrides for these crates (in addition to nixpkgs' ~60):

| Crate | What it provides |
|-------|-----------------|
| `ring` | perl for build script assembly compilation |
| `tikv-jemalloc-sys` | make for vendored jemalloc build |
| `jemalloc-sys` | make for vendored jemalloc build |
| `onig_sys` | pkg-config + oniguruma |
| `librocksdb-sys` | cmake + rocksdb |
| `zstd-sys` | pkg-config + zstd |
| `bzip2-sys` | pkg-config + bzip2 |
| `lzma-sys` | pkg-config + xz |

## Common override recipes

### openssl-sys
Covered by nixpkgs — no override needed.

### libgit2-sys
Covered by nixpkgs — no override needed.

### libz-sys
Covered by nixpkgs — no override needed.

### libsqlite3-sys
Covered by nixpkgs — no override needed.

### ring
Covered by unit2nix built-ins — no override needed.

> **Warning:** Do NOT set `RING_PREGENERATE_ASM = "1"`. That flag tells ring to *regenerate* assembly files, which fails when the source tarball's `pregenerated/` directory already exists.

### prost-build (protobuf)
Covered by nixpkgs — no override needed.

### Custom recipe template

For crates not covered by any default:

```nix
extraCrateOverrides = {
  the-sys-crate = attrs: {
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [ pkgs.the-library ];
    # Optional: env vars to control build script behavior
    THE_LIB_DIR = "${pkgs.the-library}/lib";
  };
};
```

## macOS-specific dependencies

Some crates need Apple frameworks on macOS:

```nix
extraCrateOverrides = {
  my-crate = attrs: {
    buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
      pkgs.darwin.apple_sdk.frameworks.Security
      pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
    ];
  };
};
```

## Eval-time warnings

When a crate has a `links` field but no override is configured, unit2nix emits a trace warning during Nix evaluation:

```
unit2nix: WARNING — crate 'exotic-sys' has links="exotic" but no override found.
```

These warnings are advisory — they don't fail the build. Some crates with `links` fields (like `rayon-core`) are Rust-internal and never need native overrides. unit2nix suppresses warnings for known Rust-internal crates.

## Environment variables

Many `-sys` build scripts check environment variables:

| Variable | Effect |
|----------|--------|
| `OPENSSL_DIR` | Point to OpenSSL installation |
| `LIBGIT2_NO_VENDOR=1` | Use system libgit2 |
| `LIBZ_SYS_STATIC=0` | Use shared zlib |
| `SQLITE3_LIB_DIR` | Point to SQLite lib directory |
| `ROCKSDB_LIB_DIR` | Point to RocksDB lib directory |
| `PROTOC` | Path to protobuf compiler |

## Cargo env vars

unit2nix automatically sets `CARGO_CRATE_NAME` and `CARGO_CFG_FEATURE` for every crate build. If a crate needs other `CARGO_*` env vars:

```nix
extraCrateOverrides = {
  my-crate = attrs: {
    CARGO_BIN_NAME = "my-binary";
  };
};
```

## Troubleshooting

### "pkg-config not found" or "could not find library"

Add `pkg-config` to `nativeBuildInputs` and the library to `buildInputs`:

```nix
extraCrateOverrides = {
  the-sys-crate = attrs: {
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [ pkgs.the-library ];
  };
};
```

### "failed to run custom build command"

Check the crate's `build.rs` to see what it needs, then provide via overrides.

### Build works on Linux but fails on macOS

Add Apple framework dependencies conditionally (see macOS section above).

### "linker cc not found" or linking errors

`buildRustCrate` provides a C compiler by default. If you still get errors, check that `pkg-config` is in `nativeBuildInputs` and the library provides a `.pc` file.

## Inspecting available overrides

```bash
# List nixpkgs built-in overrides
nix eval --expr 'builtins.attrNames (import <nixpkgs> {}).defaultCrateOverrides'

# List unit2nix built-in overrides
nix eval .#lib.x86_64-linux.crateOverrides --apply builtins.attrNames
```

## Full working examples

- [`tests/bat/build.nix`](../tests/bat/build.nix) — [bat](https://github.com/sharkdp/bat) (168 crates): only bat-specific macOS override needed
- [`tests/nushell/build.nix`](../tests/nushell/build.nix) — [nushell](https://github.com/nushell/nushell) (519 crates, 29 members): zero overrides
- [`tests/fd/build.nix`](../tests/fd/build.nix) — [fd](https://github.com/sharkdp/fd) (59 crates): zero overrides
- [`tests/ripgrep/build.nix`](../tests/ripgrep/build.nix) — [ripgrep](https://github.com/BurntSushi/ripgrep) (34 crates): zero overrides (pure Rust)
