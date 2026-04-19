# ─────────────────────────────────────────────────────────────────────────────
# Stage 1: Build Yew/WASM frontend with trunk
# ─────────────────────────────────────────────────────────────────────────────
FROM rust:1.85 AS builder

# Add the wasm32 compile target required by Yew.
RUN rustup target add wasm32-unknown-unknown

# Install trunk — the WASM bundler for Yew apps.
# --locked uses the exact versions in trunk's Cargo.lock for reproducibility.
RUN cargo install trunk --locked

WORKDIR /frontend

COPY . .

# Build a release WASM bundle.  Output lands in dist/.
# The API base URL is derived at runtime from the browser hostname (port 8080),
# so no compile-time API_BASE_URL is needed for the default Docker setup.
RUN trunk build --release

# ─────────────────────────────────────────────────────────────────────────────
# Stage 2: Serve static assets with nginx
# ─────────────────────────────────────────────────────────────────────────────
FROM nginx:stable-alpine

# Remove the default site so only our config is active.
RUN rm /etc/nginx/conf.d/default.conf

COPY nginx.conf /etc/nginx/conf.d/meridian.conf
COPY --from=builder /frontend/dist /usr/share/nginx/html

EXPOSE 80
