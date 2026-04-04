#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_DIR="$ROOT_DIR/backend"
FRONTEND_DIR="$ROOT_DIR/frontend"

mkdir -p "$ROOT_DIR/exports" "$ROOT_DIR/backups"

if [[ ! -f "$BACKEND_DIR/.env" && -f "$ROOT_DIR/config/default.env" ]]; then
  cp "$ROOT_DIR/config/default.env" "$BACKEND_DIR/.env"
fi

echo "==> Running backend tests"
(
  cd "$BACKEND_DIR"
  cargo test
)

echo "==> Running frontend compile check"
(
  cd "$FRONTEND_DIR"
  cargo check --target wasm32-unknown-unknown
)

echo "==> All checks completed successfully"
