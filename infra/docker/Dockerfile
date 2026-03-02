# Multi-stage Dockerfile for flipper-mcp-relay
#
# Builds only the relay crate (pure Rust, no ESP32 toolchain needed).
# The workspace Cargo.toml references a `firmware` member which requires
# the Xtensa ESP32 toolchain, so we create a dummy stub for it.
#
# Build: docker compose build (from infra/docker/)
# Or: docker build -t flipper-mcp-relay -f infra/docker/Dockerfile .

# --- Stage 1: Build ---
FROM rust:1.77-slim-bookworm AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy workspace manifest and lockfile
COPY Cargo.toml Cargo.lock ./

# Copy the relay crate
COPY relay/ relay/

# Create a dummy firmware crate so the workspace resolves
# (the real firmware crate requires the Xtensa ESP32 toolchain)
COPY firmware/Cargo.toml firmware/Cargo.toml
RUN mkdir -p firmware/src && echo "// stub for Docker build" > firmware/src/lib.rs

# Build only the relay binary with cargo cache mounts for speed
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/build/target \
    cargo build --release -p flipper-mcp-relay && \
    cp target/release/flipper-mcp-relay /usr/local/bin/flipper-mcp-relay

RUN strip /usr/local/bin/flipper-mcp-relay

# --- Stage 2: Runtime ---
FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/bin/flipper-mcp-relay /usr/local/bin/flipper-mcp-relay

EXPOSE 9090

USER nobody

ENTRYPOINT ["flipper-mcp-relay"]
CMD ["--listen", "0.0.0.0:9090"]
