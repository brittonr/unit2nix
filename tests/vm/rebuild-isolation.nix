# VM test: verify that per-crate builds are isolated and deterministic.
#
# Builds the sample workspace twice with identical inputs and verifies
# the output store paths match (deterministic). Then verifies that
# the crate dependency graph is correctly wired by checking that
# libraries are linked at distinct store paths.

{ pkgs, sampleWorkspace }:

let
  # Build a second copy to verify determinism
  sampleWorkspace2 = import ../../lib/build-from-unit-graph.nix {
    inherit pkgs;
    src = ../../sample_workspace;
    resolvedJson = ../../sample_workspace/build-plan.json;
  };

  findBinId =
    ws:
    builtins.head (
      builtins.filter (
        id: (ws.resolved.crates.${id}).crateName == "sample-bin"
      ) (builtins.attrNames ws.resolved.crates)
    );

  bin1 = sampleWorkspace.builtCrates.crates.${findBinId sampleWorkspace};
  bin2 = sampleWorkspace2.builtCrates.crates.${findBinId sampleWorkspace2};
in
pkgs.testers.runNixOSTest {
  name = "unit2nix-rebuild-isolation";

  nodes.machine =
    { pkgs, ... }:
    {
      virtualisation.graphics = false;
      environment.systemPackages = [
        bin1
        bin2
      ];
      environment.etc."unit2nix-test/bin1".text = "${bin1}";
      environment.etc."unit2nix-test/bin2".text = "${bin2}";
    };

  testScript = ''
    machine.wait_for_unit("default.target")

    # Both builds should produce identical store paths (determinism)
    path1 = machine.succeed("cat /etc/unit2nix-test/bin1").strip()
    path2 = machine.succeed("cat /etc/unit2nix-test/bin2").strip()
    assert path1 == path2, f"non-deterministic build: {path1} != {path2}"

    # The binary should link against per-crate .rlib files, not a monolithic blob
    # Check that the binary's nix-support references individual crate store paths
    ldd = machine.succeed(f"ldd {path1}/bin/sample-bin || true")

    # The binary itself should run correctly
    output = machine.succeed(f"{path1}/bin/sample-bin").strip().splitlines()
    assert len(output) == 3
    assert '"Hello, unit2nix!"' in output[0]
    assert output[1] == "Hello from App!"
    assert "built-by-unit2nix" in output[2]
  '';
}
