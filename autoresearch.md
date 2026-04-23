# Autoresearch

## Goal
Optimize nix-store caching behavior for Rust builds in unit2nix without overfitting or changing benchmark semantics.

## Primary metric
- `noop_rebuild_ms`
- Measured as warm-cache wall-clock time for `nix build .#sample --no-link`
- Best reliable result in this session family: about `59.7 ms`
- Best benchmark-bearing baseline commit: `c87f69a`

## Harness
Use Python timing harness, not hyperfine.

```bash
python3 - <<'PY'
import subprocess, time, statistics, sys
subprocess.run(["nix","build",".#sample","--no-link"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
vals=[]
for i in range(8):
    start=time.perf_counter()
    proc=subprocess.run(["nix","build",".#sample","--no-link"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if proc.returncode != 0:
        sys.exit(proc.returncode)
    vals.append((time.perf_counter()-start)*1000.0)
steady=vals[3:]
print("samples_ms=" + ",".join(f"{v:.3f}" for v in vals))
print("steady_ms=" + ",".join(f"{v:.3f}" for v in steady))
print(f"METRIC noop_rebuild_ms={statistics.mean(steady):.3f}")
PY
```

## Known bad rails
- `nix develop -c cargo test` is not a valid check in this environment.
- It fails on baseline too with linker error: `mold: fatal: unknown -m argument: 64`.
- Do not use that as acceptance gate.

## Final findings

1. Small and medium refactors in `flake.nix`, `lib/build-from-unit-graph.nix`, and `lib/fetch-source.nix` did not reliably improve the primary metric. Most regressed or landed within obvious noise.
2. Warm `nix eval` of drvPaths is much slower than warm `nix build --no-link`, and `sample` vs `sample-bin` are nearly identical. This points to global flake/output evaluation overhead, not `symlinkJoin` or sample-specific wiring, as the dominant cost under the current metric.
3. Auto-mode has a real invalidation-breadth problem: unrelated README edits are far more expensive than real source edits. Filtering/pruning the auto-mode source helped those secondary rails, but consistently hurt the primary noop metric.
4. Commit `d4988b5` is still a worthwhile correctness fix: it makes the checks import self-contained by binding `unit2nix` explicitly in `flake.nix`. However, the immediate rerun showed the performance effect was neutral/noisy, so it should not be treated as a benchmark win.

## Stop condition reached

Under the current primary metric, the explored local refactor space appears exhausted. The next useful step would require changing the objective (for example, optimize auto-mode invalidation latency directly) or pursuing a larger architectural reduction in flake evaluation surface rather than continuing small speculative refactors.

## Rules if this resumes
- Do not change benchmark workload and compare it against the noop baseline.
- Re-run promising improvements at least twice when gains are near the noise floor.
- Keep secondary README-touch/source-touch rails separate from the primary noop benchmark.
- Preserve semantics; no cheating by skipping work the real build would need.
