# Multi-stage Rust build for minimal image size
# Stage 1: Build server
FROM rust:1.85-slim-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests and benches first for layer caching
COPY api/Cargo.toml api/Cargo.lock ./
COPY api/benches ./benches

# Create dummy src to cache dependencies
RUN mkdir -p src && echo "fn main() {}" > src/main.rs
RUN cargo build --release && rm -rf src target/release/orbit-royale-server*

# Copy actual source code
COPY api/src ./src

# Build release binary
RUN cargo build --release

# Stage 2: Build cert generator
FROM rust:1.85-slim-bookworm AS cert-builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy cert generator
COPY api/scripts ./scripts

WORKDIR /app/scripts
RUN cargo build --release

# Stage 3: Runtime
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies (wget for healthcheck, openssl for certs)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    openssl \
    wget \
    && rm -rf /var/lib/apt/lists/*

# Copy server binary
COPY --from=builder /app/target/release/orbit-royale-server /app/server

# Copy cert generator
COPY --from=cert-builder /app/scripts/target/release/gen-dev-cert /app/gen-dev-cert

# Copy entrypoint script
COPY docker/api/entrypoint.sh /app/entrypoint.sh
RUN chmod +x /app/entrypoint.sh

# Create certs directory
RUN mkdir -p /app/certs

# Environment variables
ENV RUST_LOG=info
ENV CERT_PATH=/app/certs/cert.pem
ENV KEY_PATH=/app/certs/key.pem
ENV HOST=0.0.0.0
ENV PORT=4433
ENV METRICS_PORT=9090

# Expose ports
EXPOSE 4433/udp
EXPOSE 9090

# Health check using metrics endpoint
HEALTHCHECK --interval=10s --timeout=3s --start-period=10s --retries=3 \
    CMD wget --quiet --tries=1 --spider http://localhost:9090/health || exit 1

# Run entrypoint
ENTRYPOINT ["/app/entrypoint.sh"]
