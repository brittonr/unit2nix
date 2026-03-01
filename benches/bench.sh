#!/usr/bin/env bash
# unit2nix benchmarks — run with: nix develop -c ./benches/bench.sh
#
# Compares unit2nix vs crate2nix vs crane vs buildRustPackage across:
#   1. Build plan / codegen time
#   2. Nix evaluation (instantiate derivation graph)
#   3. Full build (cold, all crates)
#   4. Incremental rebuild (touch one local crate)
#   5. No-op rebuild (everything cached)
#
# Requires: hyperfine, crate2nix, nix, cargo (nightly via devshell)
# Outputs:  benches/results/ (JSON + markdown)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS="$ROOT/benches/results"
UNIT2NIX="${CARGO_TARGET_DIR:-$ROOT/target}/release/unit2nix"
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

mkdir -p "$RESULTS"

# Ensure release binary is fresh
echo "==> Building unit2nix (release)..."
cargo build --release --quiet

# ---------------------------------------------------------------------------
# Nix expressions (evaluated repeatedly by hyperfine)
# ---------------------------------------------------------------------------

NIX_UNIT2NIX="
  let
    pkgs = import <nixpkgs> {};
    ws = import $ROOT/lib/build-from-unit-graph.nix {
      inherit pkgs;
      src = $ROOT/sample_workspace;
      resolvedJson = $ROOT/sample_workspace/build-plan.json;
    };
  in ws.allWorkspaceMembers
"

NIX_CRATE2NIX="(import $ROOT/benches/crate2nix-Cargo.nix {}).allWorkspaceMembers"

NIX_CRANE="import $ROOT/benches/crane.nix { pkgs = import <nixpkgs> {}; }"

NIX_BRP="import $ROOT/benches/buildRustPackage.nix { pkgs = import <nixpkgs> {}; }"

# ===================================================================
# 1. BUILD PLAN / CODEGEN TIME
# ===================================================================
echo ""
echo "━━━ Build plan / code generation ━━━"
hyperfine \
  --warmup 2 \
  --runs 10 \
  --export-json "$RESULTS/generate.json" \
  --export-markdown "$RESULTS/generate.md" \
  --command-name "unit2nix generate (Cargo → JSON)" \
  "$UNIT2NIX --manifest-path $ROOT/sample_workspace/Cargo.toml -o $TMPDIR/plan.json 2>/dev/null" \
  --command-name "crate2nix generate (Cargo → Nix)" \
  "crate2nix generate -f $ROOT/sample_workspace/Cargo.toml -o $TMPDIR/c2n.nix -h $TMPDIR/c2n-hashes.json 2>/dev/null"

# ===================================================================
# 2. NIX EVAL — instantiate full derivation graph
# ===================================================================
echo ""
echo "━━━ Nix evaluation (nix-instantiate) ━━━"
hyperfine \
  --warmup 1 \
  --runs 10 \
  --export-json "$RESULTS/eval.json" \
  --export-markdown "$RESULTS/eval.md" \
  --command-name "unit2nix (15 per-crate drvs)" \
  "nix-instantiate --quiet --expr '$NIX_UNIT2NIX' >/dev/null 2>&1" \
  --command-name "crate2nix (16 per-crate drvs)" \
  "nix-instantiate --quiet --expr '$NIX_CRATE2NIX' >/dev/null 2>&1" \
  --command-name "crane (2 monolithic drvs)" \
  "nix-instantiate --quiet --expr '($NIX_CRANE).sample-bin' >/dev/null 2>&1" \
  --command-name "buildRustPackage (1 monolithic drv)" \
  "nix-instantiate --quiet --expr '($NIX_BRP).sample-bin' >/dev/null 2>&1"

# ===================================================================
# 3. FULL BUILD — cold build, all crates
# ===================================================================
echo ""
echo "━━━ Full build (nix-build, cold) ━━━"

# Delete output derivations to force rebuilds
echo "  Cleaning previous builds..."
for expr in \
  "$NIX_UNIT2NIX" \
  "$NIX_CRATE2NIX" \
  "($NIX_CRANE).sample-bin" \
  "($NIX_BRP).sample-bin"; do
  drv=$(nix-instantiate --quiet --expr "$expr" 2>/dev/null) || true
  [ -n "$drv" ] && nix-store --delete "$drv" 2>/dev/null || true
done

# Can't use hyperfine for cold builds (need gc between runs). Single timed runs.
echo ""
for label_expr in \
  "unit2nix:$NIX_UNIT2NIX" \
  "crate2nix:$NIX_CRATE2NIX" \
  "crane:($NIX_CRANE).sample-bin" \
  "buildRustPackage:($NIX_BRP).sample-bin"; do
  label="${label_expr%%:*}"
  expr="${label_expr#*:}"
  echo -n "  $label: "
  { time nix-build --no-out-link --expr "$expr" >/dev/null 2>&1; } 2>&1 | grep real
done

# ===================================================================
# 4. INCREMENTAL REBUILD — touch sample-lib/src/lib.rs, rebuild
# ===================================================================
echo ""
echo "━━━ Incremental rebuild (touch sample-lib, rebuild) ━━━"

# Pre-build everything so caches are warm
echo "  Pre-building all four..."
nix-build --no-out-link --expr "$NIX_UNIT2NIX" >/dev/null 2>&1
nix-build --no-out-link --expr "$NIX_CRATE2NIX" >/dev/null 2>&1
nix-build --no-out-link --expr "($NIX_CRANE).sample-bin" >/dev/null 2>&1
nix-build --no-out-link --expr "($NIX_BRP).sample-bin" >/dev/null 2>&1

# Create modified workspace copy
MODIFIED="$TMPDIR/modified_workspace"
cp -r "$ROOT/sample_workspace" "$MODIFIED"
echo "// touched" >> "$MODIFIED/sample-lib/src/lib.rs"

# Regenerate unit2nix plan for modified source
"$UNIT2NIX" --manifest-path "$MODIFIED/Cargo.toml" -o "$MODIFIED/build-plan.json" 2>/dev/null

# Regenerate crate2nix Cargo.nix for modified source
C2N_MOD="$TMPDIR/crate2nix-mod.nix"
crate2nix generate -f "$MODIFIED/Cargo.toml" -o "$C2N_MOD" -h "$TMPDIR/c2n-mod-hashes.json" 2>/dev/null

NIX_U2N_MOD="
  let
    pkgs = import <nixpkgs> {};
    ws = import $ROOT/lib/build-from-unit-graph.nix {
      inherit pkgs;
      src = $MODIFIED;
      resolvedJson = $MODIFIED/build-plan.json;
    };
  in ws.allWorkspaceMembers
"

NIX_C2N_MOD="(import $C2N_MOD {}).allWorkspaceMembers"

# Crane/BRP with modified source
CRANE_MOD="$TMPDIR/crane-mod.nix"
cat > "$CRANE_MOD" << 'NIXEOF'
{ pkgs ? import <nixpkgs> {},
  craneSrc ? builtins.fetchGit {
    url = "https://github.com/ipetkov/crane";
    rev = "8525580bc0316c39dbfa18bd09a1331e98c9e463";
  },
  src,
}:
let
  crane = import craneSrc { inherit pkgs; };
  commonArgs = { inherit src; pname = "sample-workspace"; version = "0.1.0"; strictDeps = true; };
  cargoArtifacts = crane.buildDepsOnly commonArgs;
in crane.buildPackage (commonArgs // { inherit cargoArtifacts; })
NIXEOF

BRP_MOD="$TMPDIR/brp-mod.nix"
cat > "$BRP_MOD" << 'NIXEOF'
{ pkgs ? import <nixpkgs> {}, src }:
pkgs.rustPlatform.buildRustPackage {
  pname = "sample-workspace"; version = "0.1.0";
  inherit src;
  cargoLock.lockFile = "${src}/Cargo.lock";
}
NIXEOF

echo ""
hyperfine \
  --warmup 1 \
  --runs 5 \
  --export-json "$RESULTS/incremental.json" \
  --export-markdown "$RESULTS/incremental.md" \
  --command-name "unit2nix (rebuilds: 2 of 15 crates)" \
  "nix-build --no-out-link --expr '$NIX_U2N_MOD' >/dev/null 2>&1" \
  --command-name "crate2nix (rebuilds: 3 of 16 crates)" \
  "nix-build --no-out-link --expr '$NIX_C2N_MOD' >/dev/null 2>&1" \
  --command-name "crane (rebuilds: full workspace)" \
  "nix-build --no-out-link --expr 'import $CRANE_MOD { src = $MODIFIED; }' >/dev/null 2>&1" \
  --command-name "buildRustPackage (rebuilds: full workspace)" \
  "nix-build --no-out-link --expr 'import $BRP_MOD { src = $MODIFIED; }' >/dev/null 2>&1"

# ===================================================================
# 5. NO-OP REBUILD — everything cached, measure eval + check overhead
# ===================================================================
echo ""
echo "━━━ No-op rebuild (everything cached) ━━━"
nix-build --no-out-link --expr "$NIX_UNIT2NIX" >/dev/null 2>&1
nix-build --no-out-link --expr "$NIX_CRATE2NIX" >/dev/null 2>&1
nix-build --no-out-link --expr "($NIX_CRANE).sample-bin" >/dev/null 2>&1
nix-build --no-out-link --expr "($NIX_BRP).sample-bin" >/dev/null 2>&1

hyperfine \
  --warmup 1 \
  --runs 10 \
  --export-json "$RESULTS/noop.json" \
  --export-markdown "$RESULTS/noop.md" \
  --command-name "unit2nix (15 cached drvs)" \
  "nix-build --no-out-link --expr '$NIX_UNIT2NIX' >/dev/null 2>&1" \
  --command-name "crate2nix (16 cached drvs)" \
  "nix-build --no-out-link --expr '$NIX_CRATE2NIX' >/dev/null 2>&1" \
  --command-name "crane (2 cached drvs)" \
  "nix-build --no-out-link --expr '($NIX_CRANE).sample-bin' >/dev/null 2>&1" \
  --command-name "buildRustPackage (1 cached drv)" \
  "nix-build --no-out-link --expr '($NIX_BRP).sample-bin' >/dev/null 2>&1"

# ===================================================================
# DERIVATION COUNTS
# ===================================================================
echo ""
echo "━━━ Derivation graph sizes ━━━"
U2N_DRV=$(nix-instantiate --quiet --expr "$NIX_UNIT2NIX" 2>/dev/null)
C2N_DRV=$(nix-instantiate --quiet --expr "$NIX_CRATE2NIX" 2>/dev/null)
CRANE_DRV=$(nix-instantiate --quiet --expr "($NIX_CRANE).sample-bin" 2>/dev/null)
BRP_DRV=$(nix-instantiate --quiet --expr "($NIX_BRP).sample-bin" 2>/dev/null)

U2N_RUST=$(nix-store -qR "$U2N_DRV" 2>/dev/null | grep -c "rust_.*\.drv$")
C2N_RUST=$(nix-store -qR "$C2N_DRV" 2>/dev/null | grep -c "rust_.*\.drv$")
U2N_TOTAL=$(nix-store -qR "$U2N_DRV" 2>/dev/null | grep -c '\.drv$')
C2N_TOTAL=$(nix-store -qR "$C2N_DRV" 2>/dev/null | grep -c '\.drv$')
CRANE_TOTAL=$(nix-store -qR "$CRANE_DRV" 2>/dev/null | grep -c '\.drv$')
BRP_TOTAL=$(nix-store -qR "$BRP_DRV" 2>/dev/null | grep -c '\.drv$')

echo ""
echo "| Metric | unit2nix | crate2nix | crane | buildRustPackage |"
echo "|--------|----------|-----------|-------|------------------|"
echo "| Rust crate derivations | $U2N_RUST | $C2N_RUST | 2 (deps + src) | 1 |"
echo "| Total derivation graph | $U2N_TOTAL | $C2N_TOTAL | $CRANE_TOTAL | $BRP_TOTAL |"

# ===================================================================
# SUMMARY
# ===================================================================
echo ""
echo "━━━ Results ━━━"
echo ""
for md in "$RESULTS"/*.md; do
  [ -f "$md" ] || continue
  echo "--- $(basename "$md" .md) ---"
  cat "$md"
  echo ""
done
