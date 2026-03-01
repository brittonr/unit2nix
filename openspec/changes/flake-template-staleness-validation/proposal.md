## Why

Adopting unit2nix requires too many manual steps. Users must read the README, create a flake.nix from scratch, and remember to regenerate `build-plan.json` when `Cargo.lock` changes. There's no `nix flake init` template, no staleness guard, and only one validated project (457-crate workspace). These three gaps block adoption.

## What Changes

- Add a flake template so `nix flake init -t github:brittonr/unit2nix` scaffolds a working project with unit2nix wired in
- Embed a `Cargo.lock` content hash in `build-plan.json` and check it at Nix eval time, failing with a clear message when the plan is stale
- Add the staleness hash to the Rust CLI output and the Nix consumer's validation
- Validate unit2nix against a second real-world open-source Rust workspace (ripgrep, nushell, or similar) and fix any issues found

## Capabilities

### New Capabilities
- `flake-template`: A `nix flake init` template that scaffolds a flake.nix pre-wired to use unit2nix's `buildFromUnitGraph`, with a placeholder for `build-plan.json` and instructions for generating it
- `staleness-check`: Embed a Cargo.lock hash in the build plan JSON and validate it at Nix eval time, producing a clear error when `build-plan.json` doesn't match the current `Cargo.lock`
- `real-world-validation`: Test unit2nix against a non-trivial open-source Rust workspace to shake out edge cases in source handling, feature resolution, and -sys crate overrides

### Modified Capabilities

## Impact

- `src/main.rs`: Add Cargo.lock hashing to the JSON output
- `lib/build-from-unit-graph.nix`: Add staleness check at eval time
- `flake.nix`: Register the template, expose it in flake outputs
- New `templates/` directory for the flake template
- New integration test or CI script for the real-world validation target
- README: Update quickstart to reference `nix flake init`
