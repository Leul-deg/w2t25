#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_DIR="$ROOT_DIR/backend"
FRONTEND_DIR="$ROOT_DIR/frontend"
RUST_IMAGE="${RUST_IMAGE:-rust:1.85}"
POSTGRES_HOST_PORT="${POSTGRES_HOST_PORT:-55432}"
SEEDED_DB_URL="postgres://meridian:meridian@127.0.0.1:${POSTGRES_HOST_PORT}/meridian_seeded?sslmode=disable"
INTEGRITY_DB_URL="postgres://meridian:meridian@127.0.0.1:${POSTGRES_HOST_PORT}/meridian_integrity?sslmode=disable"

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
      cargo test
      cargo check --target wasm32-unknown-unknown
    )
  else
    docker run --rm \
      -v "$ROOT_DIR:/workspace" \
      -w /workspace/frontend \
      "$RUST_IMAGE" \
      bash -lc "export PATH=/usr/local/cargo/bin:/root/.cargo/bin:\$PATH; cargo test && rustup target add wasm32-unknown-unknown >/dev/null 2>&1 && cargo check --target wasm32-unknown-unknown"
  fi
}

ensure_postgres_ready() {
  docker compose up -d postgres >/dev/null
  for _ in $(seq 1 30); do
    if docker compose exec -T postgres pg_isready -U meridian -d meridian >/dev/null 2>&1; then
      return 0
    fi
    sleep 2
  done
  echo "PostgreSQL service did not become ready in time" >&2
  return 1
}

prepare_test_databases() {
  ensure_postgres_ready
  docker compose exec -T postgres bash -lc "psql -U meridian -d meridian -tc \"SELECT 1 FROM pg_database WHERE datname='meridian_seeded'\" | grep -q 1 || psql -U meridian -d meridian -c \"CREATE DATABASE meridian_seeded;\"" >/dev/null
  docker compose exec -T postgres bash -lc "psql -U meridian -d meridian -tc \"SELECT 1 FROM pg_database WHERE datname='meridian_integrity'\" | grep -q 1 || psql -U meridian -d meridian -c \"CREATE DATABASE meridian_integrity;\"" >/dev/null
}

apply_seeded_schema() {
  for sql_file in "$ROOT_DIR"/migrations/*.sql; do
    docker compose exec -T postgres psql "$SEEDED_DB_URL" -v ON_ERROR_STOP=1 -f "/workspace/$(basename "$sql_file")" >/dev/null 2>&1 && continue
  done
}

run_backend_seed() {
  if command -v cargo >/dev/null 2>&1; then
    (
      cd "$BACKEND_DIR"
      DATABASE_URL="$SEEDED_DB_URL" cargo run --bin seed >/dev/null
    )
  else
    docker run --rm \
      --network host \
      -v "$ROOT_DIR:/workspace" \
      -w /workspace/backend \
      "$RUST_IMAGE" \
      bash -lc "export PATH=/usr/local/cargo/bin:/root/.cargo/bin:\$PATH; DATABASE_URL='$SEEDED_DB_URL' cargo run --bin seed >/dev/null"
  fi
}

run_db_backed_suites() {
  echo "==> Preparing PostgreSQL for ignored DB-backed suites"
  prepare_test_databases

  echo "==> Applying migrations to seeded database"
  if command -v psql >/dev/null 2>&1; then
    for sql_file in "$ROOT_DIR"/migrations/*.sql; do
      PGPASSWORD=meridian psql "$SEEDED_DB_URL" -v ON_ERROR_STOP=1 -f "$sql_file" >/dev/null
    done
  else
    docker run --rm \
      --network host \
      -v "$ROOT_DIR:/workspace" \
      -w /workspace \
      postgres:16 \
      bash -lc "for f in migrations/*.sql; do PGPASSWORD=meridian psql '$SEEDED_DB_URL' -v ON_ERROR_STOP=1 -f \"\$f\" >/dev/null; done"
  fi

  echo "==> Seeding test database"
  run_backend_seed

  echo "==> Running schema integrity ignored suite"
  if command -v cargo >/dev/null 2>&1; then
    (
      cd "$BACKEND_DIR"
      TEST_DATABASE_URL="$INTEGRITY_DB_URL" cargo test --test schema_integrity_tests -- --include-ignored --test-threads=1
      TEST_DATABASE_URL="$SEEDED_DB_URL" cargo test --test hardening_tests -- --include-ignored
      TEST_DATABASE_URL="$SEEDED_DB_URL" cargo test --test commerce_tests -- --include-ignored --test-threads=1
      TEST_DATABASE_URL="$SEEDED_DB_URL" cargo test --test admin_scope_tests -- --include-ignored
      TEST_DATABASE_URL="$SEEDED_DB_URL" cargo test --test api_authorization_tests -- --include-ignored
      TEST_DATABASE_URL="$SEEDED_DB_URL" cargo test --test api_auth_payload_tests -- --include-ignored
      TEST_DATABASE_URL="$SEEDED_DB_URL" cargo test --test api_products_tests -- --include-ignored
      TEST_DATABASE_URL="$SEEDED_DB_URL" cargo test --test api_notifications_payload_tests -- --include-ignored
      TEST_DATABASE_URL="$SEEDED_DB_URL" cargo test --test api_admin_users_payload_tests -- --include-ignored
      TEST_DATABASE_URL="$SEEDED_DB_URL" cargo test --test api_orders_tests -- --include-ignored
      TEST_DATABASE_URL="$SEEDED_DB_URL" cargo test --test api_config_tests -- --include-ignored
      TEST_DATABASE_URL="$SEEDED_DB_URL" cargo test --test api_logs_tests -- --include-ignored
      TEST_DATABASE_URL="$SEEDED_DB_URL" cargo test --test api_checkins_tests -- --include-ignored
      TEST_DATABASE_URL="$SEEDED_DB_URL" cargo test --test api_backups_reports_tests -- --include-ignored
      TEST_DATABASE_URL="$SEEDED_DB_URL" cargo test --test api_users_tests -- --include-ignored
      TEST_DATABASE_URL="$SEEDED_DB_URL" cargo test --test e2e_workflow_tests -- --include-ignored
      DATABASE_URL="$SEEDED_DB_URL" cargo test --bin meridian-backend -- --include-ignored
    )
  else
    docker run --rm \
      --network host \
      -v "$ROOT_DIR:/workspace" \
      -w /workspace/backend \
      "$RUST_IMAGE" \
      bash -lc "export PATH=/usr/local/cargo/bin:/root/.cargo/bin:\$PATH; \
        TEST_DATABASE_URL='$INTEGRITY_DB_URL' cargo test --test schema_integrity_tests -- --include-ignored --test-threads=1 && \
        TEST_DATABASE_URL='$SEEDED_DB_URL' cargo test --test hardening_tests -- --include-ignored && \
        TEST_DATABASE_URL='$SEEDED_DB_URL' cargo test --test commerce_tests -- --include-ignored --test-threads=1 && \
        TEST_DATABASE_URL='$SEEDED_DB_URL' cargo test --test admin_scope_tests -- --include-ignored && \
        TEST_DATABASE_URL='$SEEDED_DB_URL' cargo test --test api_authorization_tests -- --include-ignored && \
        TEST_DATABASE_URL='$SEEDED_DB_URL' cargo test --test api_auth_payload_tests -- --include-ignored && \
        TEST_DATABASE_URL='$SEEDED_DB_URL' cargo test --test api_products_tests -- --include-ignored && \
        TEST_DATABASE_URL='$SEEDED_DB_URL' cargo test --test api_notifications_payload_tests -- --include-ignored && \
        TEST_DATABASE_URL='$SEEDED_DB_URL' cargo test --test api_admin_users_payload_tests -- --include-ignored && \
        TEST_DATABASE_URL='$SEEDED_DB_URL' cargo test --test api_orders_tests -- --include-ignored && \
        TEST_DATABASE_URL='$SEEDED_DB_URL' cargo test --test api_config_tests -- --include-ignored && \
        TEST_DATABASE_URL='$SEEDED_DB_URL' cargo test --test api_logs_tests -- --include-ignored && \
        TEST_DATABASE_URL='$SEEDED_DB_URL' cargo test --test api_checkins_tests -- --include-ignored && \
        TEST_DATABASE_URL='$SEEDED_DB_URL' cargo test --test api_backups_reports_tests -- --include-ignored && \
        TEST_DATABASE_URL='$SEEDED_DB_URL' cargo test --test api_users_tests -- --include-ignored && \
        TEST_DATABASE_URL='$SEEDED_DB_URL' cargo test --test e2e_workflow_tests -- --include-ignored && \
        DATABASE_URL='$SEEDED_DB_URL' cargo test --bin meridian-backend -- --include-ignored"
  fi
}

echo "==> Running backend tests"
run_backend_tests

echo "==> Running DB-backed ignored suites"
run_db_backed_suites

echo "==> Running frontend compile check"
run_frontend_check

echo "==> All checks completed successfully"
