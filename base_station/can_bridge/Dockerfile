# Frontend build stage
FROM node:22-slim AS frontend-builder
WORKDIR /frontend
# Install pnpm
RUN corepack enable && corepack prepare pnpm@latest --activate
# Copy dependency files first for better caching
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
# Copy source and build
COPY frontend .
RUN pnpm run build

# Rust build stages
FROM rust:1.93 AS chef
# We only pay the installation cost once,
# it will be cached from the second build onwards
# Install system dependencies for serialport (libudev)
RUN apt-get update && apt-get install -y \
    libudev-dev \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare  --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release --bin can_bridge

# We do not need the Rust toolchain to run the binary!
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies (SSL and libudev for serialport)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    openssl \
    libssl-dev \
    libudev1 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/local/bin
COPY --from=builder /app/target/release/can_bridge .
COPY --from=frontend-builder /frontend/dist ./frontend/dist

ENTRYPOINT ["/usr/local/bin/can_bridge"]
