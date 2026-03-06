# All flake checks: sample workspace, override coverage, real-world validation, VM tests
{
  pkgs,
  self,
  system,
  buildFromUnitGraph,
  buildFromUnitGraphAuto,
  unit2nix,
  sampleWorkspace,
}:
{
  sample-builds = sampleWorkspace.allWorkspaceMembers;
  sample-clippy = sampleWorkspace.clippy.allWorkspaceMembers;
  sample-test-deps = sampleWorkspace.test.allWorkspaceMembers;
  sample-run-tests = sampleWorkspace.test.check."sample-lib";
  sample-run-tests-bin = sampleWorkspace.test.check."sample-bin";

  # Members filter: build only sample-bin from 4-member workspace
  sample-members-filter =
    (buildFromUnitGraph {
      inherit pkgs;
      src = ./.. + "/sample_workspace";
      resolvedJson = ./.. + "/sample_workspace/build-plan.json";
      members = [ "sample-bin" ];
    }).allWorkspaceMembers;

  # Auto mode (IFD): builds sample_workspace with no pre-generated JSON
  sample-auto =
    (buildFromUnitGraphAuto {
      inherit pkgs;
      src = ./.. + "/sample_workspace";
    }).allWorkspaceMembers;

  # Override coverage check: verify no unknown -sys crates in bat's plan
  check-overrides-bat = pkgs.runCommand "check-overrides-bat" {
    nativeBuildInputs = [
      unit2nix
      pkgs.jq
    ];
  } ''
    unit2nix --check-overrides --json -o ${./.. + "/tests/bat/build-plan.json"} > report.json
    missing=$(jq -r '.missing' report.json)
    if [ "$missing" -gt 0 ]; then
      echo "Missing overrides detected:"
      jq -r '.crates[] | select(.status == "unknown") | "  \(.name) (links=\(.links))"' report.json
      exit 1
    fi
    cp report.json $out
  '';

  # Real-world validation: pure Rust workspace (34 crates)
  validate-ripgrep = import ../tests/ripgrep/build.nix { inherit pkgs; };

  # Real-world validation: workspace with -sys crates (168 crates)
  validate-bat = import ../tests/bat/build.nix { inherit pkgs; };

  # Real-world validation: pure Rust file finder (59 crates, jemalloc)
  validate-fd = import ../tests/fd/build.nix { inherit pkgs; };

  # Real-world validation: largest test — 519 crates, 29 workspace members
  validate-nushell = import ../tests/nushell/build.nix { inherit pkgs; };
}
// pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
  # VM integration tests (Linux only — requires QEMU/KVM)
  vm-sample-bin = import ../tests/vm/sample-bin.nix {
    inherit pkgs;
    sampleBin = self.packages.${system}.sample-bin;
  };
  vm-per-crate-caching = import ../tests/vm/per-crate-caching.nix {
    inherit pkgs sampleWorkspace;
  };
  vm-rebuild-isolation = import ../tests/vm/rebuild-isolation.nix {
    inherit pkgs sampleWorkspace;
  };
}
