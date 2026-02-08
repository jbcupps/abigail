# Dev image for Abigail: Rust + Node + Tauri system deps.
# Source is bind-mounted at /app (see docker-compose.yml). Use for build/test only;
# GUI (cargo tauri dev) requires native host. See documents/HOW_TO_RUN_LOCALLY.md.

ARG RUST_VERSION=1.75
ARG NODE_VERSION=20

FROM rust:${RUST_VERSION}-bookworm
RUN apt-get update -y && apt-get install -y --no-install-recommends \
    build-essential \
    libwebkit2gtk-4.1-dev \
    libappindicator3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    patchelf \
    libssl-dev \
    libgtk-3-dev \
    pkg-config \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL https://deb.nodesource.com/setup_${NODE_VERSION}.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
ENV CARGO_TARGET_DIR=/app/target
CMD ["bash"]
