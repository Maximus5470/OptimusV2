# Stage 1: Build
FROM rust:latest AS builder

WORKDIR /app

# Copy entire workspace (simplest approach for multi-crate workspace)
COPY . .

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

# Copy config directory with language definitions
COPY --from=builder /app/config /app/config

CMD ["/app/optimus-worker"]
