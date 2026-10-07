# syntax=docker/dockerfile:1

# ---- web: build the React SPA ----
FROM node:22-bookworm-slim AS web
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# ---- server: build the Rust binary ----
FROM rust:1-bookworm AS server
WORKDIR /src/server
COPY server/ ./
RUN cargo build --release --locked && mkdir -p seed-data

# ---- runtime ----
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates poppler-utils \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home /app --shell /usr/sbin/nologin servicehub \
    && mkdir -p /data /app/seed-data \
    && chown servicehub:servicehub /data
WORKDIR /app
COPY --from=server /src/server/target/release/servicehub /usr/local/bin/servicehub
COPY --from=web /src/web/dist /app/web
# Seed CSV/JSON files (holidays, …); the directory may be empty.
COPY --from=server /src/server/seed-data /app/seed-data
ENV PORT=8080 \
    DATA_DIR=/data \
    WEB_DIST=/app/web \
    SEED_DATA_DIR=/app/seed-data \
    RUST_LOG=info,sqlx=warn
USER servicehub
VOLUME ["/data"]
EXPOSE 8080
# `serve` applies migrations on start.
CMD ["servicehub", "serve"]
