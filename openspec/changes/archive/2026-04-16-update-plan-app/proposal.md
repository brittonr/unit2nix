## Why

Every project using unit2nix has a manual regeneration step: when `Cargo.lock` changes, you must re-run unit2nix to update `build-plan.json`. Today, the staleness check catches this at build time, but the error tells you to run a raw `unit2nix` command that requires unit2nix to already be on PATH. There's no zero-setup way to regenerate.

Two gaps:
1. **Nix users** have no one-command fix — they must figure out how to get unit2nix on PATH first.
2. **Rust developers** (who may not use Nix for dev) have no idiomatic way to install/run unit2nix.

## What Changes

- **Cargo subcommand**: Ship a `cargo-unit2nix` binary (symlink or alias to `unit2nix`) so Cargo auto-discovers it as `cargo unit2nix`. Installable via `cargo install cargo-unit2nix` from crates.io.
- **Flake app**: Add `nix run .#update-plan` that runs unit2nix and writes `build-plan.json`. Ship in the flake template.
- **Staleness error**: Update the error message in `build-from-unit-graph.nix` to suggest both `nix run .#update-plan` and `cargo unit2nix`.
- **Docs**: Document both workflows in README.

## Capabilities

### New Capabilities
- `cargo-subcommand`: unit2nix is installable and runnable as `cargo unit2nix`, the idiomatic Rust developer UX
- `update-plan-app`: a flake app that regenerates `build-plan.json` with one command, shipped in the template for Nix users

### Modified Capabilities

## Impact

- `Cargo.toml`: add `[[bin]]` entries for both `unit2nix` and `cargo-unit2nix` (same source)
- `flake.nix`: new `apps` output, updated `symlinkJoin` to expose both binary names
- `templates/default/flake.nix`: new `apps` output
- `lib/build-from-unit-graph.nix`: updated staleness error message
- `README.md`: updated workflow docs covering both Nix and cargo install paths
