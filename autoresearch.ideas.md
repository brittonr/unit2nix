# Autoresearch Ideas

## Active

- Add an optional secondary benchmark for targeted local edits (touch unrelated README vs touch crate source) to measure cache invalidation breadth, but keep it separate from the primary noop metric.
- Revisit configurable local source filtering only with a secondary invalidation benchmark; prior noop-only results were noisy and not reliably better.
- Find a way to evaluate secondary auto-mode rails without contaminating the primary benchmark run. Current combined commands perturb the noop metric too much.
- Investigate flake eval overhead directly. Measurement suggests warm `nix eval` of sample drvPaths (~522–534 ms) is much slower than warm `nix build --no-link` (~72 ms), and `sample` vs `sample-bin` are nearly identical, which points away from symlinkJoin and toward shared flake/output evaluation costs.

## Strong evidence / tradeoffs

- Filtering auto-mode source in `lib/auto.nix` improved the secondary README-touch auto benchmark from about 27.3 s to about 24.6 s, but hurt the primary noop metric. Useful evidence of a tradeoff, not a kept win under the current metric.
- Post-copy pruning in auto mode appears even more promising on secondary rails:
  - prune `target`/`.direnv`/`result*` after copy: README-touch about 24.0 s
  - prune only `result*` after copy: README-touch about 23.1 s, source-touch about 1.73 s
  These are not kept wins yet because the current experiment harness contaminates the primary metric when run in the same command.
- Auto-mode unrelated README edits are far more expensive than real source edits. Baseline evidence showed roughly README-touch ~23–27 s vs source-touch ~1.7–6.0 s depending on setup, which confirms a real invalidation-breadth problem.
- `nix eval .#packages.x86_64-linux.sample.drvPath` and `sample-bin.drvPath` are both around 0.5 s warm, much slower than warm `nix build --no-link`, and nearly identical to each other. This suggests flake/output evaluation dominates over symlinkJoin differences.

## Tried / stale

- Using `pkgs.fetchgit` whenever `source.sha256` is known in `lib/fetch-source.nix` was tried together with local source filtering and did not produce a reliably better noop metric. Revisit only if paired with a broader cache-sharing metric, not the current primary one.
- Temporarily exposing `sample-auto` as a package/check target is useful only as experiment scaffolding, not itself an optimization path.
- Tiny attr-plumbing refactors in `flake.nix` / `lib/build-from-unit-graph.nix` (helper functions, hoists, alternate sample-bin projections) have all regressed the primary noop metric and are not promising under the current objective.
- Directly bypassing `cleanSourceWith` for simple local subpaths in `lib/fetch-source.nix` also regressed the primary noop metric and is not promising under this benchmark.
- Eager global precomputation in `lib/build-from-unit-graph.nix` (sources, feature maps, shared scalar maps) also regressed the primary metric and is not promising under this benchmark.
