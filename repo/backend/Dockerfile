# ─────────────────────────────────────────────────────────────────────────────
# Stage 1: Build
# ─────────────────────────────────────────────────────────────────────────────
FROM rust:1.85 AS builder

WORKDIR /workspace

# Copy workspace manifest and lockfile first so the dependency layer is
# cached independently of source changes.
COPY Cargo.toml Cargo.lock ./

# Copy source crates needed by the workspace.
# The frontend is excluded from the workspace (wasm32 target) and is not
# built here.
COPY backend/ backend/
COPY migrations/ migrations/

# Build both binaries in release mode.
# sqlx::migrate! embeds migrations at compile time; the binaries do not need
# the migrations folder at runtime.
RUN cargo build --release --manifest-path backend/Cargo.toml

# ─────────────────────────────────────────────────────────────────────────────
# Stage 2: Runtime
# ─────────────────────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

# ca-certificates: needed for TLS in outbound requests.
# postgresql-client: provides pg_dump, which the backup feature requires.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
         ca-certificates \
         postgresql-client \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /workspace/backend/target/release/meridian-backend /usr/local/bin/meridian-backend
COPY --from=builder /workspace/backend/target/release/seed              /usr/local/bin/seed

COPY backend/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh

WORKDIR /app

# Named volumes are mounted here by docker-compose.
VOLUME ["/app/exports", "/app/backups"]

EXPOSE 8080

ENTRYPOINT ["docker-entrypoint.sh"]
CMD ["meridian-backend"]
