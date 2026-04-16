## Context

Cargo auto-discovers any binary on PATH named `cargo-<name>` and makes it available as `cargo <name>`. This is how `cargo-edit`, `cargo-watch`, `cargo-expand`, and other tools work. By shipping a `cargo-unit2nix` binary, unit2nix becomes a first-class cargo subcommand with no special integration needed.

Nix flake apps (`nix run .#app-name`) are the standard way to expose project-specific commands for Nix users, requiring no installation.

Both paths serve the same goal: one-command plan regeneration with zero setup friction.

## Goals / Non-Goals

**Goals:**
- `cargo unit2nix -o build-plan.json` works after `cargo install cargo-unit2nix`
- `nix run .#update-plan` works from a fresh clone with no prior setup
- Both shipped by default (cargo subcommand in the crate, flake app in the template)
- Staleness error tells users exactly what to run

**Non-Goals:**
- Automatic regeneration during `nix build` (requires IFD, breaks pure eval)
- Git hooks or CI integration (users can add these themselves)
- Custom flags in the flake app (use `cargo unit2nix` directly for non-default features/targets)

## Decisions

### 1. Two `[[bin]]` entries in Cargo.toml

**Choice**: Add both `unit2nix` and `cargo-unit2nix` as binary targets pointing to the same `src/main.rs`.

**Why**: `cargo install cargo-unit2nix` installs both names. Users who install via Nix get `unit2nix`; users who install via cargo get `cargo unit2nix` as a subcommand. Same binary, two names, zero code duplication.

**Alternative considered**: Symlink in the Nix wrapper only. Rejected — this wouldn't help cargo-install users, and Cargo.toml `[[bin]]` is the standard way to ship cargo subcommands.

### 2. Flake app is a shell wrapper

**Choice**: `apps.update-plan` is a `writeShellScript` that runs `unit2nix --manifest-path ./Cargo.toml -o build-plan.json`.

**Why**: Hardcodes the conventional output path so users don't need to remember flags. Runs from CWD (project root), matching how every other cargo/nix command works.

### 3. Staleness error references both commands

**Choice**: The error suggests `nix run .#update-plan` first, `cargo unit2nix -o build-plan.json` second.

**Why**: Nix users hitting this error are in a Nix build context, so the Nix command is more likely to work. The cargo command is the fallback for users with it installed.

## Risks / Trade-offs

- **crates.io publishing**: The `cargo-unit2nix` name must be available on crates.io. If taken, we use a different name. → Check before publishing.
- **Two binaries, one crate**: `cargo install cargo-unit2nix` installs both `unit2nix` and `cargo-unit2nix`. Some users may find the extra binary surprising. → Acceptable, this is standard practice (e.g., `cargo-edit` installs `cargo-add`, `cargo-rm`, `cargo-upgrade`).
- **Template-only app**: Existing users won't get `nix run .#update-plan` automatically. → Document how to add it; low friction since it's a few lines of Nix.
