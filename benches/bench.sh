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

NIX_CRATE2NIX_JSON="
  (import $ROOT/benches/crate2nix-build-from-json.nix {
    pkgs = import <nixpkgs> {};
    src = $ROOT/sample_workspace;
    resolvedJson = $ROOT/benches/crate2nix-json.json;
  }).allWorkspaceMembers
"

NIX_UNIT2NIX_AUTO="
  let
    pkgs = import <nixpkgs> {};
    ws = import $ROOT/lib/auto.nix {
      inherit pkgs;
      src = $ROOT/sample_workspace;
      unit2nix = import $ROOT/benches/unit2nix-package.nix { inherit pkgs; root = $ROOT; };
    };
  in ws.allWorkspaceMembers
"

NIX_CRANE="import $ROOT/benches/crane.nix { pkgs = import <nixpkgs> {}; }"

NIX_BRP="import $ROOT/benches/buildRustPackage.nix { pkgs = import <nixpkgs> {}; }"

# ---------------------------------------------------------------------------
# cargo-nix-plugin (Nix native plugin — requires Nix 2.33)
# ---------------------------------------------------------------------------
CNP_REPO="$HOME/git/pi-repos/Mic92--cargo-nix-plugin"
CNP_PLUGIN=""
CNP_NIX233=""
HAS_CNP=false

if [ -d "$CNP_REPO" ]; then
  CNP_PLUGIN=$(nix build "$CNP_REPO#cargo-nix-plugin" --print-out-paths --no-link 2>/dev/null) || true
  CNP_NIX233=$(nix build nixpkgs#nixVersions.nix_2_33 --print-out-paths --no-link 2>/dev/null | grep -v man) || true
  if [ -n "$CNP_PLUGIN" ] && [ -n "$CNP_NIX233" ]; then
    # Quick smoke test
    if "$CNP_NIX233/bin/nix-instantiate" --quiet \
      --option plugin-files "$CNP_PLUGIN/lib/nix/plugins/libcargo_nix_plugin.so" \
      --expr "builtins.resolveCargoWorkspace { target = {}; manifestPath = \"/dev/null\"; }" \
      >/dev/null 2>&1|| true; then
      HAS_CNP=true
      echo "==> cargo-nix-plugin detected (Nix 2.33 + native plugin)"
    fi
  fi
fi

if ! $HAS_CNP; then
  echo "==> cargo-nix-plugin not available (skipping its benchmarks)"
fi

NIX_CNP="
  let
    pkgs = import <nixpkgs> {};
    cargoNix = import $CNP_REPO/lib {
      inherit pkgs;
      src = $ROOT/sample_workspace;
    };
  in cargoNix.allWorkspaceMembers
"

# Wrapper for CNP nix-instantiate (needs plugin loaded + Nix 2.33)
cnp_instantiate() {
  "$CNP_NIX233/bin/nix-instantiate" --quiet \
    --option plugin-files "$CNP_PLUGIN/lib/nix/plugins/libcargo_nix_plugin.so" \
    "$@"
}

cnp_build() {
  "$CNP_NIX233/bin/nix-build" --no-out-link \
    --option plugin-files "$CNP_PLUGIN/lib/nix/plugins/libcargo_nix_plugin.so" \
    "$@"
}

# ---------------------------------------------------------------------------
# unit2nix plugin (our own Nix native plugin — requires Nix 2.33)
# ---------------------------------------------------------------------------
U2N_PLUGIN=""
U2N_NIX233="$CNP_NIX233"  # reuse the same Nix 2.33
HAS_U2N_PLUGIN=false

U2N_PLUGIN=$(nix build "$ROOT#unit2nixPlugin" --print-out-paths --no-link 2>/dev/null) || true
if [ -z "$U2N_NIX233" ]; then
  U2N_NIX233=$(nix build nixpkgs#nixVersions.nix_2_33 --print-out-paths --no-link 2>/dev/null | grep -v man) || true
fi

if [ -n "$U2N_PLUGIN" ] && [ -n "$U2N_NIX233" ]; then
  HAS_U2N_PLUGIN=true
  echo "==> unit2nix plugin detected (Nix 2.33 + native plugin)"
else
  echo "==> unit2nix plugin not available (skipping its benchmarks)"
fi

NIX_U2N_PLUGIN="
  let
    pkgs = import <nixpkgs> {};
    ws = import $ROOT/lib/plugin.nix {
      inherit pkgs;
      src = $ROOT/sample_workspace;
    };
  in ws.allWorkspaceMembers
"

u2n_plugin_instantiate() {
  "$U2N_NIX233/bin/nix-instantiate" --quiet \
    --option plugin-files "$U2N_PLUGIN/lib/nix/plugins/libunit2nix_plugin.so" \
    "$@"
}

u2n_plugin_build() {
  "$U2N_NIX233/bin/nix-build" --no-out-link \
    --option plugin-files "$U2N_PLUGIN/lib/nix/plugins/libunit2nix_plugin.so" \
    "$@"
}

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

EVAL_ARGS=(
  --warmup 1
  --runs 10
  --export-json "$RESULTS/eval.json"
  --export-markdown "$RESULTS/eval.md"
  --command-name "unit2nix (15 per-crate drvs)"
  "nix-instantiate --quiet --expr '$NIX_UNIT2NIX' >/dev/null 2>&1"
  --command-name "unit2nix auto/IFD (15 per-crate drvs)"
  "nix-instantiate --quiet --expr '$NIX_UNIT2NIX_AUTO' >/dev/null 2>&1"
  --command-name "crate2nix (16 per-crate drvs)"
  "nix-instantiate --quiet --expr '$NIX_CRATE2NIX' >/dev/null 2>&1"
  --command-name "crate2nix --format json (15 per-crate drvs)"
  "nix-instantiate --quiet --expr '$NIX_CRATE2NIX_JSON' >/dev/null 2>&1"
)

if $HAS_CNP; then
  EVAL_ARGS+=(
    --command-name "cargo-nix-plugin (15 per-crate drvs)"
    "$CNP_NIX233/bin/nix-instantiate --quiet --option plugin-files $CNP_PLUGIN/lib/nix/plugins/libcargo_nix_plugin.so --expr '$NIX_CNP' >/dev/null 2>&1"
  )
fi

if $HAS_U2N_PLUGIN; then
  EVAL_ARGS+=(
    --command-name "unit2nix plugin (15 per-crate drvs)"
    "$U2N_NIX233/bin/nix-instantiate --quiet --option plugin-files $U2N_PLUGIN/lib/nix/plugins/libunit2nix_plugin.so --expr '$NIX_U2N_PLUGIN' >/dev/null 2>&1"
  )
fi

EVAL_ARGS+=(
  --command-name "crane (2 monolithic drvs)"
  "nix-instantiate --quiet --expr '($NIX_CRANE).sample-bin' >/dev/null 2>&1"
  --command-name "buildRustPackage (1 monolithic drv)"
  "nix-instantiate --quiet --expr '($NIX_BRP).sample-bin' >/dev/null 2>&1"
)

hyperfine "${EVAL_ARGS[@]}"

# ===================================================================
# 3. FULL BUILD — cold build, all crates
# ===================================================================
echo ""
echo "━━━ Full build (nix-build, cold) ━━━"

# Delete output derivations to force rebuilds
echo "  Cleaning previous builds..."
for expr in \
  "$NIX_UNIT2NIX" \
  "$NIX_UNIT2NIX_AUTO" \
  "$NIX_CRATE2NIX" \
  "($NIX_CRANE).sample-bin" \
  "($NIX_BRP).sample-bin" \
  "$NIX_CRATE2NIX_JSON"; do
  drv=$(nix-instantiate --quiet --expr "$expr" 2>/dev/null) || true
  [ -n "$drv" ] && nix-store --delete "$drv" 2>/dev/null || true
done

if $HAS_CNP; then
  drv=$(cnp_instantiate --expr "$NIX_CNP" 2>/dev/null) || true
  [ -n "$drv" ] && nix-store --delete "$drv" 2>/dev/null || true
fi

if $HAS_U2N_PLUGIN; then
  drv=$(u2n_plugin_instantiate --expr "$NIX_U2N_PLUGIN" 2>/dev/null) || true
  [ -n "$drv" ] && nix-store --delete "$drv" 2>/dev/null || true
fi

# Can't use hyperfine for cold builds (need gc between runs). Single timed runs.
echo ""
for label_expr in \
  "unit2nix:$NIX_UNIT2NIX" \
  "unit2nix-auto:$NIX_UNIT2NIX_AUTO" \
  "crate2nix:$NIX_CRATE2NIX" \
  "crate2nix-json:$NIX_CRATE2NIX_JSON" \
  "crane:($NIX_CRANE).sample-bin" \
  "buildRustPackage:($NIX_BRP).sample-bin"; do
  label="${label_expr%%:*}"
  expr="${label_expr#*:}"
  echo -n "  $label: "
  { time nix-build --no-out-link --expr "$expr" >/dev/null 2>&1; } 2>&1 | grep real
done

if $HAS_CNP; then
  echo -n "  cargo-nix-plugin: "
  { time cnp_build --expr "$NIX_CNP" >/dev/null 2>&1; } 2>&1 | grep real
fi

if $HAS_U2N_PLUGIN; then
  echo -n "  unit2nix-plugin: "
  { time u2n_plugin_build --expr "$NIX_U2N_PLUGIN" >/dev/null 2>&1; } 2>&1 | grep real
fi

# ===================================================================
# 4. INCREMENTAL REBUILD — touch sample-lib/src/lib.rs, rebuild
# ===================================================================
echo ""
echo "━━━ Incremental rebuild (touch sample-lib, rebuild) ━━━"

# Pre-build everything so caches are warm
echo "  Pre-building all..."
nix-build --no-out-link --expr "$NIX_UNIT2NIX" >/dev/null 2>&1
nix-build --no-out-link --expr "$NIX_UNIT2NIX_AUTO" >/dev/null 2>&1
nix-build --no-out-link --expr "$NIX_CRATE2NIX" >/dev/null 2>&1
nix-build --no-out-link --expr "$NIX_CRATE2NIX_JSON" >/dev/null 2>&1
nix-build --no-out-link --expr "($NIX_CRANE).sample-bin" >/dev/null 2>&1
nix-build --no-out-link --expr "($NIX_BRP).sample-bin" >/dev/null 2>&1
if $HAS_CNP; then
  cnp_build --expr "$NIX_CNP" >/dev/null 2>&1
fi
if $HAS_U2N_PLUGIN; then
  u2n_plugin_build --expr "$NIX_U2N_PLUGIN" >/dev/null 2>&1
fi

# Create modified workspace copy
MODIFIED="$TMPDIR/modified_workspace"
cp -r "$ROOT/sample_workspace" "$MODIFIED"
echo "// touched" >> "$MODIFIED/sample-lib/src/lib.rs"

# Regenerate unit2nix plan for modified source
"$UNIT2NIX" --manifest-path "$MODIFIED/Cargo.toml" -o "$MODIFIED/build-plan.json" 2>/dev/null

# Regenerate crate2nix Cargo.nix for modified source
C2N_MOD="$TMPDIR/crate2nix-mod.nix"
crate2nix generate -f "$MODIFIED/Cargo.toml" -o "$C2N_MOD" -h "$TMPDIR/c2n-mod-hashes.json" 2>/dev/null

# Regenerate crate2nix --format json for modified source
# NOTE: crate2nix json-output makes paths relative to the JSON file's parent,
# so the JSON must live inside the workspace for paths to resolve correctly.
C2N_JSON_MOD="$MODIFIED/resolved.json"
C2N_JSON_BIN="${CRATE2NIX_JSON_BIN:-$(which crate2nix)}"
HAS_JSON_FORMAT=false
if "$C2N_JSON_BIN" generate --help 2>&1 | grep -q -- '--format'; then
  HAS_JSON_FORMAT=true
  (cd "$MODIFIED" && "$C2N_JSON_BIN" generate --format json -o "$C2N_JSON_MOD" -h "$MODIFIED/c2n-hashes.json" 2>/dev/null)
else
  echo "  [SKIP] crate2nix --format json not available (no --format flag in $(basename "$C2N_JSON_BIN"))"
fi

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

NIX_U2N_AUTO_MOD="
  let
    pkgs = import <nixpkgs> {};
    ws = import $ROOT/lib/auto.nix {
      inherit pkgs;
      src = $MODIFIED;
      unit2nix = import $ROOT/benches/unit2nix-package.nix { inherit pkgs; root = $ROOT; };
    };
  in ws.allWorkspaceMembers
"

NIX_C2N_MOD="(import $C2N_MOD {}).allWorkspaceMembers"

NIX_C2N_JSON_MOD="
  (import $ROOT/benches/crate2nix-build-from-json.nix {
    pkgs = import <nixpkgs> {};
    src = $MODIFIED;
    resolvedJson = $C2N_JSON_MOD;
  }).allWorkspaceMembers
"

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

# NOTE: unit2nix auto/IFD is excluded from incremental benchmarks because the
# IFD plan derivation hash changes on every source edit, causing a full plan
# regeneration (~30ms) + rebuild. The incremental delta is the same (2 crates)
# but the IFD overhead makes hyperfine comparisons misleading.

echo ""

# Build incremental hyperfine args dynamically based on availability
INCR_ARGS=(
  --warmup 1
  --runs 5
  --export-json "$RESULTS/incremental.json"
  --export-markdown "$RESULTS/incremental.md"
  --command-name "unit2nix (rebuilds: 2 of 15 crates)"
  "nix-build --no-out-link --expr '$NIX_U2N_MOD' >/dev/null 2>&1"
  --command-name "crate2nix (rebuilds: 3 of 16 crates)"
  "nix-build --no-out-link --expr '$NIX_C2N_MOD' >/dev/null 2>&1"
)

if $HAS_JSON_FORMAT; then
  INCR_ARGS+=(
    --command-name "crate2nix --format json (rebuilds: 2 of 15 crates)"
    "nix-build --no-out-link --expr '$NIX_C2N_JSON_MOD' >/dev/null 2>&1"
  )
fi

NIX_CNP_MOD="
  let
    pkgs = import <nixpkgs> {};
    cargoNix = import $CNP_REPO/lib {
      inherit pkgs;
      src = $MODIFIED;
    };
  in cargoNix.allWorkspaceMembers
"

if $HAS_CNP; then
  INCR_ARGS+=(
    --command-name "cargo-nix-plugin (rebuilds: 2 of 15 crates)"
    "$CNP_NIX233/bin/nix-build --no-out-link --option plugin-files $CNP_PLUGIN/lib/nix/plugins/libcargo_nix_plugin.so --expr '$NIX_CNP_MOD' >/dev/null 2>&1"
  )
fi

NIX_U2N_PLUGIN_MOD="
  let
    pkgs = import <nixpkgs> {};
    ws = import $ROOT/lib/plugin.nix {
      inherit pkgs;
      src = $MODIFIED;
    };
  in ws.allWorkspaceMembers
"

if $HAS_U2N_PLUGIN; then
  INCR_ARGS+=(
    --command-name "unit2nix plugin (rebuilds: 2 of 15 crates)"
    "$U2N_NIX233/bin/nix-build --no-out-link --option plugin-files $U2N_PLUGIN/lib/nix/plugins/libunit2nix_plugin.so --expr '$NIX_U2N_PLUGIN_MOD' >/dev/null 2>&1"
  )
fi

INCR_ARGS+=(
  --command-name "crane (rebuilds: full workspace)"
  "nix-build --no-out-link --expr 'import $CRANE_MOD { src = $MODIFIED; }' >/dev/null 2>&1"
  --command-name "buildRustPackage (rebuilds: full workspace)"
  "nix-build --no-out-link --expr 'import $BRP_MOD { src = $MODIFIED; }' >/dev/null 2>&1"
)

hyperfine "${INCR_ARGS[@]}"

# ===================================================================
# 5. NO-OP REBUILD — everything cached, measure eval + check overhead
# ===================================================================
echo ""
echo "━━━ No-op rebuild (everything cached) ━━━"
nix-build --no-out-link --expr "$NIX_UNIT2NIX" >/dev/null 2>&1
nix-build --no-out-link --expr "$NIX_UNIT2NIX_AUTO" >/dev/null 2>&1
nix-build --no-out-link --expr "$NIX_CRATE2NIX" >/dev/null 2>&1
nix-build --no-out-link --expr "$NIX_CRATE2NIX_JSON" >/dev/null 2>&1
nix-build --no-out-link --expr "($NIX_CRANE).sample-bin" >/dev/null 2>&1
nix-build --no-out-link --expr "($NIX_BRP).sample-bin" >/dev/null 2>&1
if $HAS_CNP; then
  cnp_build --expr "$NIX_CNP" >/dev/null 2>&1
fi
if $HAS_U2N_PLUGIN; then
  u2n_plugin_build --expr "$NIX_U2N_PLUGIN" >/dev/null 2>&1
fi

NOOP_ARGS=(
  --warmup 1
  --runs 10
  --export-json "$RESULTS/noop.json"
  --export-markdown "$RESULTS/noop.md"
  --command-name "unit2nix (15 cached drvs)"
  "nix-build --no-out-link --expr '$NIX_UNIT2NIX' >/dev/null 2>&1"
  --command-name "unit2nix auto/IFD (15 cached drvs)"
  "nix-build --no-out-link --expr '$NIX_UNIT2NIX_AUTO' >/dev/null 2>&1"
  --command-name "crate2nix (16 cached drvs)"
  "nix-build --no-out-link --expr '$NIX_CRATE2NIX' >/dev/null 2>&1"
  --command-name "crate2nix --format json (15 cached drvs)"
  "nix-build --no-out-link --expr '$NIX_CRATE2NIX_JSON' >/dev/null 2>&1"
)

if $HAS_CNP; then
  NOOP_ARGS+=(
    --command-name "cargo-nix-plugin (15 cached drvs)"
    "$CNP_NIX233/bin/nix-build --no-out-link --option plugin-files $CNP_PLUGIN/lib/nix/plugins/libcargo_nix_plugin.so --expr '$NIX_CNP' >/dev/null 2>&1"
  )
fi

if $HAS_U2N_PLUGIN; then
  NOOP_ARGS+=(
    --command-name "unit2nix plugin (15 cached drvs)"
    "$U2N_NIX233/bin/nix-build --no-out-link --option plugin-files $U2N_PLUGIN/lib/nix/plugins/libunit2nix_plugin.so --expr '$NIX_U2N_PLUGIN' >/dev/null 2>&1"
  )
fi

NOOP_ARGS+=(
  --command-name "crane (2 cached drvs)"
  "nix-build --no-out-link --expr '($NIX_CRANE).sample-bin' >/dev/null 2>&1"
  --command-name "buildRustPackage (1 cached drv)"
  "nix-build --no-out-link --expr '($NIX_BRP).sample-bin' >/dev/null 2>&1"
)

hyperfine "${NOOP_ARGS[@]}"

# ===================================================================
# DERIVATION COUNTS
# ===================================================================
echo ""
echo "━━━ Derivation graph sizes ━━━"
U2N_DRV=$(nix-instantiate --quiet --expr "$NIX_UNIT2NIX" 2>/dev/null)
U2N_A_DRV=$(nix-instantiate --quiet --expr "$NIX_UNIT2NIX_AUTO" 2>/dev/null)
C2N_DRV=$(nix-instantiate --quiet --expr "$NIX_CRATE2NIX" 2>/dev/null)
C2N_J_DRV=$(nix-instantiate --quiet --expr "$NIX_CRATE2NIX_JSON" 2>/dev/null)
CRANE_DRV=$(nix-instantiate --quiet --expr "($NIX_CRANE).sample-bin" 2>/dev/null)
BRP_DRV=$(nix-instantiate --quiet --expr "($NIX_BRP).sample-bin" 2>/dev/null)

U2N_RUST=$(nix-store -qR "$U2N_DRV" 2>/dev/null | grep -c "rust_.*\.drv$")
U2N_A_RUST=$(nix-store -qR "$U2N_A_DRV" 2>/dev/null | grep -c "rust_.*\.drv$")
C2N_RUST=$(nix-store -qR "$C2N_DRV" 2>/dev/null | grep -c "rust_.*\.drv$")
C2N_J_RUST=$(nix-store -qR "$C2N_J_DRV" 2>/dev/null | grep -c "rust_.*\.drv$")
U2N_TOTAL=$(nix-store -qR "$U2N_DRV" 2>/dev/null | grep -c '\.drv$')
U2N_A_TOTAL=$(nix-store -qR "$U2N_A_DRV" 2>/dev/null | grep -c '\.drv$')
C2N_TOTAL=$(nix-store -qR "$C2N_DRV" 2>/dev/null | grep -c '\.drv$')
C2N_J_TOTAL=$(nix-store -qR "$C2N_J_DRV" 2>/dev/null | grep -c '\.drv$')
CRANE_TOTAL=$(nix-store -qR "$CRANE_DRV" 2>/dev/null | grep -c '\.drv$')
BRP_TOTAL=$(nix-store -qR "$BRP_DRV" 2>/dev/null | grep -c '\.drv$')

CNP_RUST="—"
CNP_TOTAL="—"
if $HAS_CNP; then
  CNP_DRV=$(cnp_instantiate --expr "$NIX_CNP" 2>/dev/null)
  CNP_RUST=$(nix-store -qR "$CNP_DRV" 2>/dev/null | grep -c "rust_.*\.drv$")
  CNP_TOTAL=$(nix-store -qR "$CNP_DRV" 2>/dev/null | grep -c '\.drv$')
fi

U2NP_RUST="—"
U2NP_TOTAL="—"
if $HAS_U2N_PLUGIN; then
  U2NP_DRV=$(u2n_plugin_instantiate --expr "$NIX_U2N_PLUGIN" 2>/dev/null)
  U2NP_RUST=$(nix-store -qR "$U2NP_DRV" 2>/dev/null | grep -c "rust_.*\.drv$")
  U2NP_TOTAL=$(nix-store -qR "$U2NP_DRV" 2>/dev/null | grep -c '\.drv$')
fi

echo ""
echo "| Metric | unit2nix | unit2nix auto | unit2nix plugin | crate2nix | crate2nix json | cargo-nix-plugin | crane | buildRustPackage |"
echo "|--------|----------|---------------|-----------------|-----------|----------------|------------------|-------|------------------|"
echo "| Rust crate derivations | $U2N_RUST | $U2N_A_RUST | $U2NP_RUST | $C2N_RUST | $C2N_J_RUST | $CNP_RUST | 2 (deps + src) | 1 |"
echo "| Total derivation graph | $U2N_TOTAL | $U2N_A_TOTAL | $U2NP_TOTAL | $C2N_TOTAL | $C2N_J_TOTAL | $CNP_TOTAL | $CRANE_TOTAL | $BRP_TOTAL |"

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
