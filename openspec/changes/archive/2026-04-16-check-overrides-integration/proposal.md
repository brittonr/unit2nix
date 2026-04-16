# Check-Overrides Integration

## Problem

`--check-overrides` exists as a standalone CLI flag but isn't wired into the normal workflow. Users must know to run it manually, and the flake template doesn't mention it. The result:

1. Users generate a build plan, try to build, hit a cryptic `-sys` crate failure, *then* discover `--check-overrides` exists
2. The `update-plan` app doesn't run the check automatically after regeneration
3. The flake template has no pre-build check or CI hook
4. The check output goes to stdout but isn't structured for machine consumption (e.g., a flake check)

## Solution

1. **Auto-check after generation**: When `unit2nix` writes a build plan (via `-o`), automatically run the override check and print a summary. Make it skippable with `--no-check`.
2. **Flake check integration**: Add a `checks.${system}.overrides` derivation that reads the build plan and fails if any `-sys` crate has no known override.
3. **Template update**: Include the override check in the scaffolded `flake.nix` and `update-plan` app.
4. **Structured output**: Add `--check-overrides --json` for machine-readable override reports.

## Value

- Users discover missing overrides *before* a 10-minute build fails
- CI catches override regressions automatically
- The happy path (generate → check → build) is the default, not opt-in
