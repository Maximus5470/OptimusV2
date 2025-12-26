# Stage 1: Build
FROM rust:latest AS builder

WORKDIR /app

# Copy entire workspace (simplest approach for multi-crate workspace)
COPY . .

# Build the API binary
RUN cargo build -p optimus-api --release

# Stage 2: Runtime
FROM debian:bookworm-slim

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/optimus-api /app/optimus-api

# Copy config directory with language definitions
COPY --from=builder /app/config /app/config

# Expose port (default 8080)
EXPOSE 8080

CMD ["/app/optimus-api"]
