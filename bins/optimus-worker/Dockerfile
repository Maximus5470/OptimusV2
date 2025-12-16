# Stage 1: Build
FROM rust:1.76-slim AS builder

WORKDIR /app

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY bins/optimus-worker ./bins/optimus-worker
COPY libs/optimus-common ./libs/optimus-common

# Build the worker binary
RUN cargo build -p optimus-worker --release

# Stage 2: Runtime
FROM debian:bookworm-slim

WORKDIR /app

# Install Docker CLI (for Bollard to work)
RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Copy the binary from builder
COPY --from=builder /app/target/release/optimus-worker /app/optimus-worker

CMD ["/app/optimus-worker"]
