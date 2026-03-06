# Proposal: Default `-o` to `build-plan.json`

## Problem

Every consumer of unit2nix writes the same wrapper script:

```bash
exec unit2nix --manifest-path ./Cargo.toml -o build-plan.json
```

Since `--manifest-path` already defaults to `./Cargo.toml`, the wrapper
exists solely to pass `-o build-plan.json`. This boilerplate appears in
three places today:

1. **unit2nix's own flake** — `apps.update-plan`
2. **The template** — `apps.update-plan` (identical copy)
3. **Every downstream consumer** — e.g. drift's `updatePlan` devShell
   script *and* `apps.update-plan` (two copies in one flake)

All four are the same script.

## Proposal

Default `-o` to `build-plan.json` when no output is specified:

```rust
// cli.rs — before
#[arg(short, long)]
pub output: Option<PathBuf>,

// cli.rs — after
#[arg(short, long, default_value = "build-plan.json")]
pub output: PathBuf,
```

Add `--stdout` for piping:

```rust
/// Write to stdout instead of a file
#[arg(long)]
pub stdout: bool,
```

In `run.rs`, the write logic becomes:

```rust
if cli.stdout {
    println!("{json}");
} else {
    std::fs::write(&cli.output, &json)?;
    eprintln!("Wrote {}", cli.output.display());
}
```

## What this eliminates

**Downstream flakes** drop the wrapper entirely. Drift's flake goes from:

```nix
updatePlan = pkgs.writeShellScriptBin "update-plan" ''
  exec ${unit2nix.packages.${system}.unit2nix}/bin/unit2nix \
    --manifest-path ./Cargo.toml \
    -o build-plan.json
'';
# ... plus apps.update-plan with the same script
```

To just adding the package to the devShell:

```nix
nativeBuildInputs = [
  unit2nix.packages.${system}.unit2nix
];
```

Users run `unit2nix` — done. No wrapper, no app, no duplication.

**unit2nix's own flake** drops `apps.update-plan` entirely.

**The template** simplifies to documenting `unit2nix` in the shellHook
instead of recreating the wrapper.

## Migration

- Existing `-o build-plan.json` invocations keep working (explicit value
  matches the new default).
- Scripts piping stdout switch to `--stdout` (or keep `-o /dev/stdout`).
- `--check-overrides` already requires `-o` to point at an existing file,
  so the default of `build-plan.json` is the right thing there too.

## Scope

- `src/cli.rs` — change `output` default, add `--stdout` flag
- `src/run.rs` — update write logic
- `flake.nix` — remove `apps.update-plan`
- `templates/default/flake.nix` — remove `apps.update-plan`, simplify
  devShell docs
- `README.md` — update usage examples
