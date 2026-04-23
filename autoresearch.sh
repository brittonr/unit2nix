#!/usr/bin/env bash
set -euo pipefail

python3 - <<'PY'
import subprocess, time, statistics, sys
subprocess.run(["nix","build",".#sample","--no-link"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
vals=[]
for _ in range(8):
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
