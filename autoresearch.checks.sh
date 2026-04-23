#!/usr/bin/env bash
set -euo pipefail

nix build .#sample --no-link >/dev/null
