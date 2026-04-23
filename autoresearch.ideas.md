# Autoresearch Ideas

- Add configurable local source filtering so workspace-root crates stop hashing unrelated files into per-crate Nix sources.
- Use `pkgs.fetchgit` whenever `source.sha256` is known in `lib/fetch-source.nix` so git sources become first-class fixed-output store paths.
- Filter auto-mode resolution input in `lib/auto.nix` instead of `cp -r ${src} source` to keep IFD cache keys stable when docs and other non-Cargo files change.
- Add an optional benchmark for targeted local edits (touch unrelated README vs touch crate source) to measure cache invalidation breadth, not just noop rebuild time.
