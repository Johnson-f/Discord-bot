## Multi-stage build for the Discord bot
# Uses a slim Debian base to satisfy font/rendering libs required by font-kit/image

FROM rust:1.82-slim-bullseye AS builder

ENV CARGO_TERM_COLOR=always
WORKDIR /app

# System deps for building and for font/image rendering
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config build-essential ca-certificates libssl-dev \
    libfontconfig1-dev libfreetype6-dev libexpat1-dev \
    && rm -rf /var/lib/apt/lists/*

# Pre-copy manifests to leverage Docker layer caching
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY models ./models

# Release build
RUN cargo build --release

# Runtime image
FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libfontconfig1 libfreetype6 libexpat1 fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m bot

WORKDIR /app

COPY --from=builder /app/target/release/Discord-bot /usr/local/bin/discord-bot

USER bot
ENV RUST_LOG=info

# Entrypoint
CMD ["discord-bot"]

