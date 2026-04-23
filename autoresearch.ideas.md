# Autoresearch Ideas

## Active

- Add an optional secondary benchmark for targeted local edits (touch unrelated README vs touch crate source) to measure cache invalidation breadth, but keep it separate from the primary noop metric.
- Revisit configurable local source filtering only with a secondary invalidation benchmark; prior noop-only results were noisy and not reliably better.
- Explore whether auto-mode source filtering can be made cheaper at steady state (for example by filtering only selected top-level noise paths, or pruning after copy instead of using a full filtered source path) so it keeps most of the ~10% secondary rail gain without hurting the primary noop metric.
- Compare unrelated-file invalidation against real source-edit invalidation to prove any filtering win is targeting noise rather than legitimate rebuild work.

## Promising but not primary-metric winners

- Filtering auto-mode source in `lib/auto.nix` improved the secondary README-touch auto benchmark from about 27.3 s to about 24.6 s, but hurt the primary noop metric. Useful evidence of a tradeoff, not a kept win under the current metric.

## Tried / stale

- Using `pkgs.fetchgit` whenever `source.sha256` is known in `lib/fetch-source.nix` was tried together with local source filtering and did not produce a reliably better noop metric. Revisit only if paired with a broader cache-sharing metric, not the current primary one.
- Temporarily exposing `sample-auto` as a package/check target is useful only as experiment scaffolding, not itself an optimization path.
