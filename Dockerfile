# ---- Builder stage ----
FROM rust:latest AS builder

WORKDIR /app

# Copy manifests first for dependency caching
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY src/ src/

RUN cargo build --release

# ---- Runtime stage ----
FROM debian:trixie-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        chromium \
        git \
    && rm -rf /var/lib/apt/lists/*

# chromiumoxide looks for "chromium" or "chromium-browser" on PATH
ENV CHROME_PATH=/usr/bin/chromium

WORKDIR /app

COPY --from=builder /app/target/release/skyclaw ./skyclaw
COPY skyclaw.toml ./skyclaw.toml
COPY entrypoint.sh ./entrypoint.sh
# SOUL.md is loaded from ray-workspace (git-synced, editable)
RUN chmod +x ./entrypoint.sh

ENV TELEGRAM_BOT_TOKEN=""
ENV GITHUB_TOKEN=""

EXPOSE 8080

ENTRYPOINT ["./entrypoint.sh"]
