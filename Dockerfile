# Multi-stage Docker build for production deployments

# Build stage
FROM rust:1.75-slim as builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy Cargo files first for better caching
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Build release binary
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 trader

WORKDIR /home/trader

# Copy binary from builder
COPY --from=builder /app/target/release/opinion-hft ./

# Copy configuration
COPY config.toml ./
COPY --chown=trader:trader .env.production .env

# Create data directories
RUN mkdir -p data logs \
    && chown -R trader:trader data logs

# Switch to non-root user
USER trader

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=10s --retries=3 \
    CMD ./scripts/healthcheck.sh || exit 1

# Expose metrics endpoint (optional)
EXPOSE 8080

# Set environment
ENV RUST_LOG=info
ENV RUST_BACKTRACE=1

CMD ["./opinion-hft"]
