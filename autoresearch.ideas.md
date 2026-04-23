# Autoresearch Closeout

## Final conclusions

- Best reliable primary benchmark result in this session family remained the warm-cache baseline around `59.7 ms` for `nix build .#sample --no-link`.
- The `d4988b5` checks-import change is worth keeping as a correctness fix, but its performance effect was neutral/noisy after immediate rerun confirmation.
- Warm `nix eval` of drvPaths (`sample`, `sample-bin`, and especially `unit2nix`) is far more expensive than warm `nix build --no-link`, which points to global flake/output evaluation as the dominant overhead under this benchmark.
- Sample-specific packaging differences (`sample` vs `sample-bin`, `allWorkspaceMembers` vs single build, small `flake.nix` shape tweaks) did not materially improve the metric.
- Auto-mode has a real invalidation-breadth problem: unrelated README edits are much more expensive than real source edits. Several auto-mode filtering/pruning ideas improved that secondary rail, but all regressed the primary metric.

## Future work if this resumes

- Change the optimization target before resuming. Two viable next objectives:
  - optimize auto-mode invalidation latency as the primary metric, or
  - pursue a larger architectural reduction in flake evaluation surface.
- If keeping the current primary metric, avoid more tiny attr-plumbing or eager precompute experiments; they consistently regressed.
- Keep secondary README-touch/source-touch rails separate from the primary noop benchmark to avoid contaminating the metric.

## Retired paths

- `pkgs.fetchgit` + local-source-filter combinations in `lib/fetch-source.nix` under the current primary metric.
- Tiny attr-plumbing refactors in `flake.nix` / `lib/build-from-unit-graph.nix` (helper functions, hoists, alternate sample-bin projections, packaging `checksArgs`, factoring `samplePackages`, replacing `pick`).
- Directly bypassing `cleanSourceWith` for simple local subpaths in `lib/fetch-source.nix`.
- Eager global precomputation in `lib/build-from-unit-graph.nix` (sources, feature maps, shared scalar maps).
- Treating checks-path cleanup as a likely optimization source; it is better viewed as correctness/maintenance work.
