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

  # Regression: public workspace test attrs stay cycle-safe with dev-dep cycles
  test-attrs-cycle = import ../tests/test-attrs-cycle/build.nix { inherit pkgs; };

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

  # Flake-parts module test: verify the module produces correct outputs
  flake-parts-module = import ../tests/flake-parts/build.nix { inherit pkgs self; };

  # Overlay smoke test: build sample workspace via pkgs.unit2nix overlay
  overlay-smoke =
    let
      overlayedPkgs = pkgs.extend (import ../nix/overlay.nix { inherit self; });
      ws = overlayedPkgs.unit2nix.buildFromUnitGraph {
        src = ./.. + "/sample_workspace";
        resolvedJson = ./.. + "/sample_workspace/build-plan.json";
      };
    in
    ws.allWorkspaceMembers;

  # Real-world validation: pure Rust workspace (34 crates)
  validate-ripgrep = import ../tests/ripgrep/build.nix { inherit pkgs; };

  # Real-world validation: workspace with -sys crates (168 crates)
  validate-bat = import ../tests/bat/build.nix { inherit pkgs; };

  # Real-world validation: pure Rust file finder (59 crates, jemalloc)
  validate-fd = import ../tests/fd/build.nix { inherit pkgs; };

  # Real-world validation: largest test — 519 crates, 29 workspace members
  validate-nushell = import ../tests/nushell/build.nix { inherit pkgs; };
}
// pkgs.lib.optionalAttrs (pkgs.stdenv.isLinux && pkgs.stdenv.isx86_64) {
  # Cross-compilation: x86_64 → aarch64 via pkgsCross
  validate-cross-aarch64 = import ../tests/cross/build.nix {
    inherit pkgs unit2nix;
  };
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
