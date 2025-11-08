# Use cargo-chef for dependency caching
FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

# Prepare the dependency recipe
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Build dependencies - this layer is cached when dependencies don't change
FROM chef AS builder

# Install cargo-leptos
RUN cargo install --locked cargo-leptos

# Build dependencies (cached layer)
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Copy source and build application
COPY . .
RUN cargo leptos build --release

# Runtime image
FROM debian:bookworm-slim AS runtime
WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy the binary from builder
COPY --from=builder /app/target/release/discord-verify /usr/local/bin/discord-verify

# Copy Leptos frontend assets
COPY --from=builder /app/target/site /app/target/site

EXPOSE 3000

CMD ["discord-verify"]
