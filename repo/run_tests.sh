#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_DIR="$ROOT_DIR/backend"
FRONTEND_DIR="$ROOT_DIR/frontend"
RUST_IMAGE="${RUST_IMAGE:-rust:1.85}"

mkdir -p "$ROOT_DIR/exports" "$ROOT_DIR/backups"

if [[ ! -f "$BACKEND_DIR/.env" && -f "$ROOT_DIR/config/default.env" ]]; then
  cp "$ROOT_DIR/config/default.env" "$BACKEND_DIR/.env"
fi

run_backend_tests() {
  if command -v cargo >/dev/null 2>&1; then
    (
      cd "$BACKEND_DIR"
      cargo test
    )
  else
    docker run --rm \
      -v "$ROOT_DIR:/workspace" \
      -w /workspace/backend \
      "$RUST_IMAGE" \
      bash -lc "export PATH=/usr/local/cargo/bin:/root/.cargo/bin:\$PATH; cargo test"
  fi
}

run_frontend_check() {
  if command -v cargo >/dev/null 2>&1; then
    (
      cd "$FRONTEND_DIR"
      cargo check --target wasm32-unknown-unknown
    )
  else
    docker run --rm \
      -v "$ROOT_DIR:/workspace" \
      -w /workspace/frontend \
      "$RUST_IMAGE" \
      bash -lc "export PATH=/usr/local/cargo/bin:/root/.cargo/bin:\$PATH; rustup target add wasm32-unknown-unknown >/dev/null 2>&1 && cargo check --target wasm32-unknown-unknown"
  fi
}

echo "==> Running backend tests"
run_backend_tests

echo "==> Running frontend compile check"
run_frontend_check

echo "==> All checks completed successfully"
