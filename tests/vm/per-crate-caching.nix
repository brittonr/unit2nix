# VM test: verify per-crate Nix caching works correctly.
#
# Builds the sample workspace, then verifies that individual crate
# derivations exist as separate store paths — proving that changing
# one crate won't invalidate others.

{ pkgs, sampleWorkspace }:

pkgs.testers.runNixOSTest {
  name = "unit2nix-per-crate-caching";

  nodes.machine =
    { pkgs, ... }:
    {
      virtualisation.graphics = false;

      # Install all workspace members
      environment.systemPackages = [ sampleWorkspace.allWorkspaceMembers ];

      # Also copy individual crate derivations into the store so we can inspect them
      environment.etc."unit2nix-test/crate-paths".text =
        let
          crateIds = builtins.attrNames sampleWorkspace.resolved.crates;
          paths = map (
            id:
            let
              info = sampleWorkspace.resolved.crates.${id};
            in
            "${info.crateName}=${sampleWorkspace.builtCrates.crates.${id}}"
          ) crateIds;
        in
        builtins.concatStringsSep "\n" paths;
    };

  testScript = ''
    machine.wait_for_unit("default.target")

    # Read the per-crate store paths
    crate_paths_raw = machine.succeed("cat /etc/unit2nix-test/crate-paths").strip()
    crate_paths = {}
    for line in crate_paths_raw.splitlines():
        name, path = line.split("=", 1)
        crate_paths[name] = path

    # Verify we have the expected number of crates (15 for sample workspace)
    count = len(crate_paths)
    assert count == 15, f"expected 15 crates, got {count}"

    # Verify each crate has its own unique store path
    unique_paths = set(crate_paths.values())
    assert len(unique_paths) == count, f"expected {count} unique paths, got {len(unique_paths)}"

    # Verify all paths are valid nix store paths
    for name, path in crate_paths.items():
        machine.succeed(f"test -e {path}")

    # Verify specific crates exist as separate derivations
    for expected in ["serde", "serde_json", "sample-lib", "sample-macro", "sample-bin"]:
        assert expected in crate_paths, f"missing crate: {expected}"

    # Verify the sample-lib and serde paths are DIFFERENT store paths
    # (proving they're separate derivations, not one blob)
    assert crate_paths["sample-lib"] != crate_paths["serde"], \
        "sample-lib and serde should be separate store paths"

    # Verify the binary still works after all this
    output = machine.succeed("sample-bin").strip().splitlines()
    assert len(output) == 3
  '';
}
