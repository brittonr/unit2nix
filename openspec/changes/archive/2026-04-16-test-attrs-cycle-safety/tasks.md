## 1. Rewire public test attrs

- [x] 1.1 Change `test.workspaceMembers.<name>.build` to use per-member `mkTestGraphForCrate` results instead of `testCrates`
- [x] 1.2 Change `test.allWorkspaceMembers` to aggregate per-member test builds instead of the shared all-members test graph
- [x] 1.3 Verify zero remaining references to `mkTestBuiltByPkgs`/`testCrates`, then remove them if no consumer remains; otherwise document the retained internal-only use

## 2. Add regression coverage

- [x] 2.1 Add a minimal workspace fixture/check whose dev-dependency cycle would recurse under the legacy shared graph
- [x] 2.2 Register the regression in `nix/checks.nix` so CI evaluates the affected public test attrs on that fixture
- [x] 2.3 Re-verify existing sample test attrs after the rewire (`sample-test-deps`, `sample-run-tests`, `sample-run-tests-bin`) and confirm `test.check` still follows the same per-member graph model

## 3. Document and verify

- [x] 3.1 Update README/docs comments to state that public workspace test attrs use per-member graphs and are cycle-safe
- [x] 3.2 Run targeted verification for the rewired attrs and confirm both the new cyclic check and existing sample test checks stay green
