#!/usr/bin/env bash
# Start, stop, or inspect a local Pointer deployment cell.
# Does not call npm. See docs/zh-CN/deploy/quick-start.md.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if ! command -v node >/dev/null 2>&1; then
  echo "quick-deploy: node is required" >&2
  exit 1
fi

exec node "$ROOT/scripts/quick-deploy.mjs" "$@"
