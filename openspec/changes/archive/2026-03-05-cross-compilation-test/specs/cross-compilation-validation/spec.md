## ADDED Requirements

### Requirement: Cross-compiled build plan generation
The CLI SHALL accept `--target <TRIPLE>` and produce a build plan where the `target` field matches the specified triple. Cargo's platform-specific dependency filtering SHALL be reflected in the output (only crates needed for the target platform are included).

#### Scenario: Target triple stored in JSON output
- **WHEN** `unit2nix --target aarch64-unknown-linux-gnu` is run against the sample workspace
- **THEN** the resulting JSON contains `"target": "aarch64-unknown-linux-gnu"`

#### Scenario: Target triple propagates through merge
- **WHEN** `merge()` is called with `target = Some("aarch64-unknown-linux-gnu")`
- **THEN** the output `NixBuildPlan.target` is `Some("aarch64-unknown-linux-gnu")`

### Requirement: Cross-compiled Nix build produces target-architecture binaries
The Nix build system SHALL produce binaries for the target platform when given a cross-compiled build plan and cross-compilation pkgs. Binary crates SHALL be ELF aarch64 when targeting `aarch64-unknown-linux-gnu`.

#### Scenario: Sample workspace builds for aarch64
- **WHEN** a build plan is generated with `--target aarch64-unknown-linux-gnu` and built with `pkgsCross.aarch64-multiplatform`
- **THEN** `sample-bin` output is an `ELF 64-bit` `ARM aarch64` executable

#### Scenario: Build completes without errors
- **WHEN** the cross build is evaluated and built
- **THEN** all 4 workspace members (sample-lib, sample-macro, sample-bin, sample-build-script) build successfully

### Requirement: Proc-macros and build scripts run on build platform
During cross-compilation, proc-macro crates and build script dependencies SHALL be compiled for the build platform (x86_64), not the target platform (aarch64). This is required because proc-macros and build scripts execute at compile time on the build machine.

#### Scenario: Proc-macro crate compiles during cross build
- **WHEN** `sample-bin` (which depends on `sample-macro`, a proc-macro) is cross-compiled for aarch64
- **THEN** the build succeeds, proving `sample-macro` was compiled for x86_64 (the build platform) and executed during compilation

#### Scenario: Build script executes during cross build
- **WHEN** `sample-build-script` (which has a build.rs) is cross-compiled for aarch64
- **THEN** the build succeeds, proving the build script ran on x86_64 during compilation

### Requirement: Target mismatch warning
The Nix evaluation SHALL emit a trace warning when the build plan's target triple does not match `pkgs.stdenv.hostPlatform.config`. The warning SHALL NOT fire when the target matches.

#### Scenario: Mismatch warning fires on wrong pkgs
- **WHEN** an x86_64 build plan (no `--target`) is evaluated with `pkgsCross.aarch64-multiplatform`
- **THEN** a `builtins.trace` warning containing "build plan target" and "differs from" is emitted

#### Scenario: No warning when target matches
- **WHEN** an aarch64 build plan is evaluated with `pkgsCross.aarch64-multiplatform`
- **THEN** no target mismatch warning is emitted
