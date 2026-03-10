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
        curl \
        wget \
        python3 \
        python3-pip \
        python3-venv \
        jq \
        unzip \
        build-essential \
        procps \
        less \
        vim \
        ssh \
        rsync \
    && rm -rf /var/lib/apt/lists/*

# Install GitHub CLI (gh)
RUN curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
        | dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg \
    && chmod go+r /usr/share/keyrings/githubcli-archive-keyring.gpg \
    && echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
        | tee /etc/apt/sources.list.d/github-cli.list > /dev/null \
    && apt-get update \
    && apt-get install -y gh \
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
