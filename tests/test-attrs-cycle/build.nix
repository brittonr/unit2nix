# Regression fixture for workspace test attrs with a dev-dependency cycle.
#
# Graph:
#   cycle-a [dev-dep] -> cycle-b -> cycle-c -> cycle-a
#
# The old shared test graph recursed when aggregate/public test attrs forced
# every workspace member into one dev-dependency fixpoint. The cycle-safe
# per-member graph model should keep these attrs evaluable.
{
  pkgs ? import <nixpkgs> { },
}:
let
  buildFromUnitGraph = import ../../lib/build-from-unit-graph.nix;

  ws = buildFromUnitGraph {
    inherit pkgs;
    src = ./.;
    resolvedJson = ./build-plan.json;
    skipStalenessCheck = true;
  };
in
pkgs.runCommand "test-attrs-cycle" { } ''
  test -e ${ws.test.allWorkspaceMembers}
  test -e ${ws.test.workspaceMembers."cycle-a".build}
  test -e ${ws.test.workspaceMembers."cycle-b".build}
  touch "$out"
''
