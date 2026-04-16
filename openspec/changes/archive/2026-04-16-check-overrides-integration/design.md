# Check-Overrides Integration — Design

## Overview

Wire `--check-overrides` into the default workflow so users discover missing overrides before a build fails. Three integration points: post-generation auto-check, flake check derivation, and structured JSON output.

## 1. Auto-check after generation

### Current flow
```
unit2nix -o build-plan.json  →  writes JSON  →  exits
```

### New flow
```
unit2nix -o build-plan.json  →  writes JSON  →  runs override check  →  prints summary  →  exits
```

### Implementation

In `run.rs`, after writing the JSON output, call `overrides::check_overrides(&plan)` automatically. The check is purely informational (exit code 0 even if missing overrides are found — it's a warning, not an error).

Add `--no-check` flag to skip the auto-check (for scripted/CI usage where the check is run separately).

### Output

Same format as current `--check-overrides`, but preceded by a separator:

```
Wrote build-plan.json

Override check:
  ✓ ring           (covered — needs perl for build script)
  ? my-custom-sys  (unknown — may need extraCrateOverrides)

Summary: 1 covered, 1 may need attention
```

## 2. Flake check derivation

### New check

Add `checks.${system}.overrides` to the flake template and sample workspace:

```nix
checks.overrides = pkgs.runCommand "check-overrides" {} ''
  ${unit2nix}/bin/unit2nix --check-overrides -o ${./build-plan.json} --json > $out
  # Fail if any crate has status "unknown" or "missing"
  if ${pkgs.jq}/bin/jq -e '.missing > 0' $out; then
    echo "Missing overrides detected. Run: unit2nix --check-overrides -o build-plan.json"
    exit 1
  fi
'';
```

This makes `nix flake check` catch override regressions automatically.

### Auto mode

For `buildFromUnitGraphAuto`, the check runs inside the IFD derivation after unit2nix generates the plan. Missing overrides are printed as warnings (can't fail the IFD without blocking the build).

## 3. Structured JSON output

### Flag

```
--check-overrides --json
```

### Output format

```json
{
  "total": 5,
  "covered": 3,
  "noOverrideNeeded": 1,
  "missing": 1,
  "crates": [
    {
      "name": "ring",
      "links": "ring_core_0_17_14_",
      "status": "covered",
      "note": "needs perl for build script assembly compilation"
    },
    {
      "name": "my-custom-sys",
      "links": "my_custom",
      "status": "unknown",
      "note": null
    }
  ]
}
```

### Use cases

- CI can parse JSON and fail on `missing > 0`
- Tooling can generate override stubs from the `unknown` entries
- Dashboards can track override coverage across projects

## 4. Template updates

### `templates/default/flake.nix`

Add the override check to the `update-plan` app:

```bash
unit2nix --manifest-path ./Cargo.toml -o build-plan.json
# Auto-check runs after generation (see terminal output for override status)
```

Add a commented-out `checks.overrides` block with a note about enabling it.

### README

Update the "Keeping build-plan.json up to date" section to mention the auto-check.

## Backward compatibility

- `--check-overrides` alone (no `--json`) still produces the human-readable table format
- `--no-check` only applies to the auto-check after generation
- No change to exit codes: override warnings don't cause non-zero exit
