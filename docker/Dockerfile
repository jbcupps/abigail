# AO Build & Validation Container
#
# Multi-stage Dockerfile that:
#   Stage 1 (builder): Compiles the full AO workspace and runs tests
#   Stage 2 (runtime): Minimal image with compiled artifacts for validation
#
# Note: AO is primarily a desktop GUI app (Tauri/webview). This container
# is for build validation, CI, and test execution. A headless runtime
# exposing an HTTP API is planned for a future iteration.
#
# Build:   docker build -f docker/Dockerfile -t ao-build .
# Test:    docker run --rm ao-build cargo test --all
#
# Ref: https://v2.tauri.app/start/prerequisites/#linux

# ═══════════════════════════════════════════════════════════════════════════
# Stage 1: Builder
# ═══════════════════════════════════════════════════════════════════════════
FROM rust:1.84-bookworm AS builder

ENV DEBIAN_FRONTEND=noninteractive

# System dependencies for Tauri 2 on Linux + build tools
RUN apt-get update && apt-get install -y --no-install-recommends \
    libwebkit2gtk-4.1-dev \
    libappindicator3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    patchelf \
    libssl-dev \
    libgtk-3-dev \
    build-essential \
    curl \
    pkg-config \
    file \
    libxcb-render0-dev \
    libxcb-shape0-dev \
    libxcb-xfixes0-dev \
    libxkbcommon-dev \
    && rm -rf /var/lib/apt/lists/*

# Node.js 20 for frontend build
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy workspace files
COPY . .

# Build frontend
RUN cd tauri-app/src-ui && npm install && npm run build

# Build all Rust crates (release mode)
RUN cargo build --release

# Run tests
RUN cargo test --all

# ═══════════════════════════════════════════════════════════════════════════
# Stage 2: Runtime (minimal validation image)
# ═══════════════════════════════════════════════════════════════════════════
FROM debian:bookworm-slim AS runtime

ENV DEBIAN_FRONTEND=noninteractive

# Minimal runtime deps
RUN apt-get update && apt-get install -y --no-install-recommends \
    libwebkit2gtk-4.1-0 \
    libayatana-appindicator3-1 \
    libssl3 \
    libgtk-3-0 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Non-root user for security
RUN useradd -m -s /bin/bash ao
USER ao

WORKDIR /app

# Copy compiled binaries from builder
COPY --from=builder /build/target/release/ao-keygen /app/ao-keygen

# Default: show help / version info
CMD ["echo", "AO build validation image. Use 'docker run --rm ao-build cargo test --all' with the builder stage for testing."]
