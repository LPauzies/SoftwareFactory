#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "=================================================================="
echo " WAGNER // Autonomous Software Factory (Rust Axum + Vue 3 + agy)  "
echo "=================================================================="

cleanup() {
  echo ""
  echo "[WAGNER] Shutting down services..."
  kill $(jobs -p) 2>/dev/null || true
}
trap cleanup EXIT INT TERM

echo "[1/2] Starting Rust Orchestrator Backend on http://127.0.0.1:3001 ..."
(cd "$ROOT_DIR/backend" && cargo run --quiet) &

echo "[2/2] Starting WAGNER UI on http://localhost:8888 ..."
(cd "$ROOT_DIR/frontend" && npm run dev) &

wait
