## 1. Auto-check after plan generation

- [x] 1.1 Add `--no-check` flag to `src/cli.rs` — skips the post-generation override check
- [x] 1.2 In `run.rs`, after writing the build plan JSON, call `overrides::check_overrides(&plan)` unless `--no-check` is set
- [x] 1.3 Add a visual separator before the check output ("Override coverage:" header)
- [x] 1.4 Verify: `unit2nix -o plan.json` on sample workspace shows the check summary
- [x] 1.5 Verify: `unit2nix -o plan.json --no-check` suppresses the check

## 2. Structured JSON output

- [x] 2.1 Add `--json` flag to `src/cli.rs` — when combined with `--check-overrides`, outputs JSON instead of human-readable table
- [x] 2.2 Define `OverrideReport` struct in `src/overrides.rs`: `{ total, covered, no_override_needed, missing, crates: Vec<CrateOverrideStatus> }`
- [x] 2.3 Define `CrateOverrideStatus` struct: `{ name, links, status: "covered"|"no-override-needed"|"unknown", note: Option<String> }`
- [x] 2.4 Refactor `check_overrides` to return `OverrideReport` instead of printing directly
- [x] 2.5 Add `print_override_report(report, json: bool)` — handles both human and JSON output
- [x] 2.6 Wire `--json` flag through `run_check_overrides`

## 3. Flake check derivation

- [x] 3.1 Add `checks.${system}.check-overrides-bat` to main flake — runs `unit2nix --check-overrides --json` on bat's plan, fails on missing > 0
- [x] 3.2 Verify: `nix flake check` passes (14 checks, 0 missing overrides in bat)
- [x] 3.3 Bat plan used instead of all targets (ripgrep/fd/nushell have their own validation checks)

## 4. Template updates

- [x] 4.1 Update `templates/default/flake.nix` — add commented-out `checks.overrides` block
- [x] 4.2 Update `update-plan` app in template — add note about auto-check output

## 5. Tests

- [x] 5.1 Unit test: `check_overrides` returns correct report for plan with ring (covered) + unknown sys crate
- [x] 5.2 Unit test: `check_overrides` returns empty report for pure Rust plan (no links crates)
- [x] 5.3 Unit test: JSON output is valid and parseable
- [x] 5.4 `cargo test` — 44/44 pass
- [x] 5.5 `cargo clippy` — 0 warnings
- [x] 5.6 `nix flake check` — 14/14 pass (including new check-overrides-bat)

## 6. Documentation

- [x] 6.1 Update README "Keeping build-plan.json up to date" section — mention auto-check
- [x] 6.2 Update README CLI section — document `--no-check`, `--json`, `--check-overrides`, `--include-dev` flags
- [x] 6.3 Update `docs/sys-crate-overrides.md` — document flake check integration, auto-check, JSON output
- [x] 6.4 Update napkin with session notes
