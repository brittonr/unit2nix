# Autoresearch

## Goal
Optimize nix-store caching behavior for Rust builds in unit2nix without overfitting or changing benchmark semantics.

## Primary metric
- `noop_rebuild_ms`
- Measured as warm-cache wall-clock time for `nix build .#sample --no-link`
- Current usable baseline from `411a48f`: about 88 ms steady-state

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

## Current hypotheses
1. `lib/auto.nix` broad `cp -r ${src} source` likely harms cacheability more than fetch-source tweaks.
2. `lib/fetch-source.nix` local-source filtering may help unrelated-file invalidation, but benefit on noop metric is noisy and not yet proven.
3. README-touch or unrelated-file-touch benchmarks are useful as secondary evidence, but not primary metric.
4. Auto-mode experiments need a secondary rail that does not redefine the primary harness; temporarily exposing `sample-auto` is acceptable for measurement, but do not mix auto warmup into the primary noop metric.
5. Filtering auto-mode source did improve the secondary README-touch auto rail (~27.3 s -> ~24.6 s), but regressed the primary noop metric. This likely reflects a real tradeoff between auto invalidation breadth and steady-state noop overhead.
6. Post-copy pruning may be a better direction than eval-time filtered sources. Early evidence suggests pruning only `result*` helps secondary auto rails substantially, but the current experiment shape still contaminates the primary metric when secondary rails run in the same command.

## Rules
- Do not change benchmark workload and compare it against noop baseline.
- Re-run promising improvements at least twice when gains are near noise floor.
- Prefer repo-native Nix checks if a correctness rail is needed.
- Preserve semantics; no cheating by skipping work the real build would need.
