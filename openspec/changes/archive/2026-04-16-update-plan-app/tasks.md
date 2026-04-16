## 1. Cargo subcommand

- [x] 1.1 Add `[[bin]] name = "cargo-unit2nix"` to `Cargo.toml` pointing to `src/main.rs`
- [x] 1.2 Verify `cargo run --bin cargo-unit2nix -- -o /dev/null --manifest-path sample_workspace/Cargo.toml` works
- [x] 1.3 Update the Nix `symlinkJoin` wrapper to expose both `unit2nix` and `cargo-unit2nix`

## 2. Flake app

- [x] 2.1 Add `apps.<system>.update-plan` to `flake.nix` — shell wrapper that runs `unit2nix --manifest-path ./Cargo.toml -o build-plan.json`
- [x] 2.2 Verify `nix run .#update-plan` works from the repo root

## 3. Flake template

- [x] 3.1 Add `apps.<system>.update-plan` to `templates/default/flake.nix` using the unit2nix input
- [x] 3.2 Add comment explaining when to run it

## 4. Staleness error

- [x] 4.1 Update the error message in `lib/build-from-unit-graph.nix` to suggest `nix run .#update-plan` (primary) and `cargo unit2nix -o build-plan.json` (secondary)

## 5. Documentation

- [x] 5.1 Add installation section to README covering both `cargo install cargo-unit2nix` and Nix
- [x] 5.2 Update quickstart to mention `nix run .#update-plan` for regeneration
- [x] 5.3 Add a "Keeping build-plan.json up to date" section
