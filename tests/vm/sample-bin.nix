# VM test: verify sample-bin built by unit2nix runs correctly in a NixOS VM.
#
# Tests that the binary produced by per-crate buildRustCrate derivations
# runs and produces correct output for all 3 features:
# 1. serde JSON serialization (library + feature gate)
# 2. proc-macro derive (HelloMacro)
# 3. build script env var (GENERATED_VALUE)

{ pkgs, sampleBin }:

pkgs.testers.runNixOSTest {
  name = "unit2nix-sample-bin";

  nodes.machine =
    { ... }:
    {
      virtualisation.graphics = false;
      environment.systemPackages = [ sampleBin ];
    };

  testScript = ''
    import json

    machine.wait_for_unit("default.target")

    # Binary exists on PATH
    machine.succeed("which sample-bin")

    # Run and capture all 3 lines of output
    output = machine.succeed("sample-bin").strip()
    lines = output.splitlines()
    assert len(lines) == 3, f"expected 3 output lines, got {len(lines)}: {lines}"

    # Line 1: serde JSON — proves library dep + feature gating works
    parsed = json.loads(lines[0])
    assert parsed == {"message": "Hello, unit2nix!"}, f"serde output wrong: {parsed}"

    # Line 2: proc-macro derive — proves proc-macro cross-compilation works
    assert lines[1] == "Hello from App!", f"proc-macro output wrong: {lines[1]}"

    # Line 3: build script env var — proves build.rs ran and set GENERATED_VALUE
    assert lines[2] == "build-script says: built-by-unit2nix", f"build-script output wrong: {lines[2]}"
  '';
}
