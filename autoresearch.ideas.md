# Autoresearch Ideas

## Active

- Filter auto-mode resolution input in `lib/auto.nix` instead of `cp -r ${src} source` to keep IFD cache keys stable when docs and other non-Cargo files change.
- Add an optional secondary benchmark for targeted local edits (touch unrelated README vs touch crate source) to measure cache invalidation breadth, but keep it separate from the primary noop metric.
- Revisit configurable local source filtering only with a secondary invalidation benchmark; prior noop-only results were noisy and not reliably better.

## Tried / stale

- Using `pkgs.fetchgit` whenever `source.sha256` is known in `lib/fetch-source.nix` was tried together with local source filtering and did not produce a reliably better noop metric. Revisit only if paired with a broader cache-sharing metric, not the current primary one.
