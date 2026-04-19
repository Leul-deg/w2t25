#!/bin/sh
set -e

# Seed the database (runs all SQLx migrations then upserts the reference data).
# This is idempotent — safe to run every time the container starts.
echo "==> Running seed (migrations + reference data)..."
seed

echo "==> Starting meridian-backend..."
exec "$@"
