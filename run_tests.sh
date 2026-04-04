#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$ROOT_DIR/repo"

echo "==> Running backend tests"
(
  cd "$REPO_DIR/backend"
  cargo test
)

echo "==> Running frontend compile check"
(
  cd "$REPO_DIR/frontend"
  cargo check --target wasm32-unknown-unknown
)

echo "==> All checks completed successfully"
