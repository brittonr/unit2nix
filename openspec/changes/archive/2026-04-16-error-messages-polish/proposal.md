# Error Messages Polish

## Problem

unit2nix has several failure modes where the error messages are technically correct but not actionable. Users hit these during initial setup or when their project has unusual characteristics, and the messages don't clearly explain what went wrong or how to fix it.

Key pain points identified from validation sessions:

1. **Stale build plan** — the Nix-side `builtins.throw` shows expected/got hashes but could also show the actual command to run
2. **Missing `-sys` override** — the `builtins.trace` warning at eval time is easy to miss; build failure happens later with a cryptic `buildRustCrate` error
3. **`nix-prefetch-git` failure** — when the tool isn't on PATH or the repo is unreachable, the error is a raw process error
4. **Unknown source type** — `parse_source` returns an error but the context about which crate / what source string is sometimes lost
5. **Auto mode git dep without hash** — `builtins.throw` message is correct but could include the exact `nix-prefetch-git` command to run
6. **`cargo build --unit-graph` failure** — stderr from cargo is forwarded but truncated; the stdout snippet in the error may not show the root cause

## Solution

Audit every error path in both Rust CLI and Nix consumers. For each, ensure the message includes:
- **What** failed (concrete noun: "crate X", "file Y")  
- **Why** it failed (the actual error, not just "failed")
- **How to fix** it (exact command or config change)

## Value

- Reduces time-to-first-build for new users
- Eliminates "what does this error mean?" issues
- Makes unit2nix more self-documenting
