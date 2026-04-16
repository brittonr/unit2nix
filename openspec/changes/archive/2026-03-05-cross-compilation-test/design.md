## Context

unit2nix has cross-compilation support wired through three layers:
1. **CLI**: `--target <TRIPLE>` passes to `cargo build --unit-graph --target <TRIPLE>`, stored in JSON output
2. **Nix build**: proc-macros and build scripts route to `self.build.crates` (build platform), normal deps to `self.crates` (target platform)
3. **Nix validation**: `_targetCheck` warns when build plan target ≠ `pkgs.stdenv.hostPlatform.config`

None of this is tested. The sample workspace has a proc-macro (`sample-macro`), build script (`sample-build-script`), library, and binary — sufficient to exercise all cross-compilation concerns without -sys crate complexity.

## Goals / Non-Goals

**Goals:**
- Validate that `unit2nix --target aarch64-unknown-linux-gnu` produces a correct build plan
- Validate that building with `pkgsCross.aarch64-multiplatform` produces aarch64 binaries
- Validate proc-macros execute on the build platform (x86_64) during cross builds
- Validate the target mismatch warning fires when plan target ≠ host platform
- Add a Rust unit test confirming `--target` propagates to the JSON `target` field
- Keep tests fast and self-contained (use sample workspace, not external projects)

**Non-Goals:**
- Testing cross-compilation with -sys crates (platform-specific native deps are an override concern, not a unit2nix concern)
- Testing non-Linux targets (e.g., macOS, Windows) — would require additional CI infrastructure
- Testing auto mode cross-compilation (IFD + cross adds complexity; manual mode is the documented path)
- Testing aarch64 → x86_64 (reverse direction) — symmetric; one direction proves the machinery

## Decisions

**Use `pkgsCross.aarch64-multiplatform` on x86_64-linux**: This is the most common cross target, well-supported in nixpkgs, and doesn't require additional toolchain setup. Alternative considered: RISC-V — less commonly used, more likely to have nixpkgs issues unrelated to unit2nix.

**Generate the cross build plan in the test via IFD rather than checking it in**: A checked-in `build-plan-aarch64.json` would drift from the sample workspace. Using IFD in the test keeps it always in sync. The test itself is for the cross build, not for avoiding IFD. Alternative considered: checked-in JSON — simpler but creates maintenance burden.

**Validate binary architecture via `file` command**: `file $out/bin/sample-bin` reports ELF target architecture. This is the most direct assertion — no need to run the binary (can't execute aarch64 on x86_64 without qemu-user). Alternative considered: `readelf -h` — more detailed but `file` is simpler and sufficient.

**Single flake check, not a VM test**: Cross builds don't need a running VM — we just need to verify the derivation builds and produces the right architecture. VM tests are for runtime behavior. This keeps CI fast.

## Risks / Trade-offs

**[Cross builds are slow]** → Only build the sample workspace (4 small crates), not a large project. Cross builds add ~2-3x overhead vs native but the sample is tiny.

**[IFD in tests requires nightly Rust]** → The test derivation needs `cargo --unit-graph` which requires nightly. Pass the flake's rust toolchain through to the IFD build. Same pattern as `sample-auto`.

**[nixpkgs cross toolchain breakage]** → Cross compilation in nixpkgs occasionally breaks. If the test becomes flaky due to upstream issues, it can be gated behind `pkgs.stdenv.isLinux && pkgs.stdenv.isx86_64` or skipped with a comment.

**[`file` output format varies]** → Pin the check to `ELF 64-bit LSB` + `ARM aarch64` which is stable across nixpkgs versions.
