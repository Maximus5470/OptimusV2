# Stage 1: Build
FROM rust:1.76-slim AS builder

WORKDIR /app

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY bins/optimus-api ./bins/optimus-api
COPY libs/optimus-common ./libs/optimus-common

# Build the API binary
RUN cargo build -p optimus-api --release

# Stage 2: Runtime
FROM debian:bookworm-slim

WORKDIR /app

# Copy the binary from builder
COPY --from=builder /app/target/release/optimus-api /app/optimus-api

# Expose port (default 8080)
EXPOSE 8080

CMD ["/app/optimus-api"]
