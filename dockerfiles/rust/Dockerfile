# Rust Execution Environment - Optimized for Code Execution
FROM rust:1.75-slim

# Set environment variables for performance
ENV CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:$PATH \
    RUSTFLAGS="-C opt-level=2 -C debuginfo=0"

WORKDIR /code

# Install required packages
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy runner script
COPY runner.sh /code/runner.sh
RUN chmod +x /code/runner.sh

# Create non-root user for security
RUN useradd -m -u 1000 optimus && \
    chown -R optimus:optimus /code

USER optimus

# Set entrypoint to runner script
ENTRYPOINT ["/code/runner.sh"]
