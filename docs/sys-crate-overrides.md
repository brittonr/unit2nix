# Overriding -sys crates

Rust `-sys` crates wrap C libraries. They compile fine with `cargo build` because your system has the headers and libraries installed globally, but Nix sandbox builds have no global state — every dependency must be declared explicitly.

unit2nix uses `buildRustCrate` under the hood, which supports per-crate overrides via `defaultCrateOverrides`. This guide covers how to write them.

## How it works

When a crate has a `links` field in its `Cargo.toml` (e.g., `links = "git2"`), Cargo tells rustc about the native library. unit2nix passes this through to `buildRustCrate`, which uses it for `-L` link paths. But the C library itself must be provided — that's what overrides do.

```nix
ws = buildFromUnitGraph {
  inherit pkgs src;
  resolvedJson = ./build-plan.json;
  defaultCrateOverrides = pkgs.defaultCrateOverrides // {
    # Keys are crate names (not package IDs)
    libgit2-sys = attrs: {
      nativeBuildInputs = [ pkgs.pkg-config ];
      buildInputs = [ pkgs.libgit2 ];
    };
  };
};
```

The override function receives the crate's attributes and returns an attrset that gets merged in. The three fields you'll use most:

| Field | Purpose | Example |
|-------|---------|---------|
| `nativeBuildInputs` | Tools needed at build time (runs on host) | `pkg-config`, `cmake`, `protobuf` |
| `buildInputs` | Libraries needed at build + link time | `openssl.dev`, `zlib`, `libgit2` |
| Environment variables | Control build script behavior | `LIBGIT2_NO_VENDOR = "1"` |

## Common recipes

### openssl-sys

```nix
openssl-sys = attrs: {
  nativeBuildInputs = [ pkgs.pkg-config ];
  buildInputs = [ pkgs.openssl.dev ];
};
```

### libgit2-sys

```nix
libgit2-sys = attrs: {
  nativeBuildInputs = [ pkgs.pkg-config ];
  buildInputs = [ pkgs.libgit2 ];
  # Use system libgit2 instead of vendored build (avoids needing cmake)
  LIBGIT2_NO_VENDOR = "1";
};
```

### libz-sys (zlib)

```nix
libz-sys = attrs: {
  nativeBuildInputs = [ pkgs.pkg-config ];
  buildInputs = [ pkgs.zlib ];
  # Use system zlib instead of building from source
  LIBZ_SYS_STATIC = "0";
};
```

### libsqlite3-sys

```nix
libsqlite3-sys = attrs: {
  nativeBuildInputs = [ pkgs.pkg-config ];
  buildInputs = [ pkgs.sqlite.dev ];
};
```

### prost-build (protobuf)

```nix
prost-build = attrs: {
  nativeBuildInputs = [ pkgs.protobuf ];
};
```

### rdkafka-sys

```nix
rdkafka-sys = attrs: {
  nativeBuildInputs = [ pkgs.pkg-config ];
  buildInputs = [ pkgs.rdkafka ];
};
```

### librocksdb-sys

```nix
librocksdb-sys = attrs: {
  nativeBuildInputs = [ pkgs.cmake ];
  buildInputs = [ pkgs.rocksdb ];
  ROCKSDB_LIB_DIR = "${pkgs.rocksdb}/lib";
};
```

### ring (special case)

`ring` isn't a `-sys` crate but needs a C compiler for its assembly routines. It usually works without overrides because `buildRustCrate` provides a C compiler by default, but if you hit issues:

```nix
ring = attrs: {
  nativeBuildInputs = [ pkgs.perl ];  # needed for its build script
};
```

> **Warning:** Do NOT set `RING_PREGENERATE_ASM = "1"`. That flag tells ring to *regenerate* assembly files, which fails when the source tarball's `pregenerated/` directory already exists. The source tarball ships pre-generated assembly — just let it use those.

## macOS-specific dependencies

Some crates need Apple frameworks on macOS:

```nix
my-crate = attrs: {
  buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
    pkgs.darwin.apple_sdk.frameworks.Security
    pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
  ];
};
```

## Combining multiple overrides

Stack overrides for crates that need several things:

```nix
defaultCrateOverrides = pkgs.defaultCrateOverrides // {
  openssl-sys = attrs: {
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [ pkgs.openssl.dev ];
  };
  libgit2-sys = attrs: {
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [ pkgs.libgit2 ];
    LIBGIT2_NO_VENDOR = "1";
  };
  libz-sys = attrs: {
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [ pkgs.zlib ];
    LIBZ_SYS_STATIC = "0";
  };
};
```

## Nixpkgs built-in overrides

`pkgs.defaultCrateOverrides` already includes overrides for common crates (curl-sys, openssl-sys, etc.). By using `pkgs.defaultCrateOverrides // { ... }`, you inherit those and only need to add project-specific ones. Check what's already covered:

```bash
nix eval --expr 'builtins.attrNames (import <nixpkgs> {}).defaultCrateOverrides'
```

## Environment variables

Many `-sys` build scripts check environment variables to decide whether to vendor or use system libraries:

| Variable | Effect |
|----------|--------|
| `OPENSSL_DIR` | Point to OpenSSL installation |
| `LIBGIT2_NO_VENDOR=1` | Use system libgit2 |
| `LIBZ_SYS_STATIC=0` | Use shared zlib |
| `SQLITE3_LIB_DIR` | Point to SQLite lib directory |
| `ROCKSDB_LIB_DIR` | Point to RocksDB lib directory |
| `PROTOC` | Path to protobuf compiler |

Set these in your override:

```nix
my-sys-crate = attrs: {
  SOME_LIB_DIR = "${pkgs.some-lib}/lib";
};
```

## Troubleshooting

### "pkg-config not found" or "could not find library"

Add `pkg-config` to `nativeBuildInputs` and the library to `buildInputs`:

```nix
the-sys-crate = attrs: {
  nativeBuildInputs = [ pkgs.pkg-config ];
  buildInputs = [ pkgs.the-library ];
};
```

### "failed to run custom build command"

The build script needs a tool or library. Check the crate's `build.rs` to see what it looks for, then provide it via `nativeBuildInputs` (tools) or `buildInputs` (libraries).

### "cmake not found"

Some vendoring build scripts use cmake. Either provide it or disable vendoring:

```nix
the-sys-crate = attrs: {
  nativeBuildInputs = [ pkgs.cmake ];
};
```

### Build works on Linux but fails on macOS

Add Apple framework dependencies conditionally:

```nix
the-crate = attrs: {
  buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
    pkgs.darwin.apple_sdk.frameworks.Security
  ];
};
```

### "linker cc not found" or linking errors

`buildRustCrate` provides a C compiler by default. If you still get linker errors, the library's `.pc` file or `-L` path may not be propagated. Check that `pkg-config` is in `nativeBuildInputs` and the library package provides a `.pc` file (usually the `.dev` output: `pkgs.openssl.dev`, not `pkgs.openssl`).

## Cargo env vars

unit2nix automatically sets `CARGO_CRATE_NAME` and `CARGO_CFG_FEATURE` for every crate build. These aren't provided by `buildRustCrate` upstream but are needed by some crates:

- `CARGO_CRATE_NAME` — the crate name with `-` replaced by `_` (used by `env!("CARGO_CRATE_NAME")` at compile time)
- `CARGO_CFG_FEATURE` — comma-separated list of enabled features (used by build scripts via `std::env::var("CARGO_CFG_FEATURE")`)

If a crate needs other `CARGO_*` env vars that aren't set, provide them via overrides:

```nix
my-crate = attrs: {
  CARGO_BIN_NAME = "my-binary";  # if the crate reads this at compile time
};
```

## Full working examples

- [`tests/bat/build.nix`](../tests/bat/build.nix) — [bat](https://github.com/sharkdp/bat) (168 crates) with overrides for `libgit2-sys` and `libz-sys`
- [`tests/nushell/build.nix`](../tests/nushell/build.nix) — [nushell](https://github.com/nushell/nushell) (519 crates, 29 workspace members) with overrides for `libsqlite3-sys` and `ring`
- [`tests/fd/build.nix`](../tests/fd/build.nix) — [fd](https://github.com/sharkdp/fd) (59 crates) with `tikv-jemalloc-sys` vendored build
