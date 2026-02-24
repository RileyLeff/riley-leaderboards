FROM rust:1.88 AS builder
WORKDIR /app

# Copy manifests first for dependency caching
COPY Cargo.toml Cargo.lock ./
COPY crates/riley-leaderboards-core/Cargo.toml crates/riley-leaderboards-core/Cargo.toml
COPY crates/riley-leaderboards-api/Cargo.toml crates/riley-leaderboards-api/Cargo.toml
COPY crates/riley-leaderboards-cli/Cargo.toml crates/riley-leaderboards-cli/Cargo.toml

# Create stub source files so cargo can resolve and download dependencies
RUN mkdir -p crates/riley-leaderboards-core/src && \
    echo "pub fn stub() {}" > crates/riley-leaderboards-core/src/lib.rs && \
    mkdir -p crates/riley-leaderboards-api/src && \
    echo "pub fn stub() {}" > crates/riley-leaderboards-api/src/lib.rs && \
    mkdir -p crates/riley-leaderboards-cli/src && \
    echo "fn main() {}" > crates/riley-leaderboards-cli/src/main.rs

# Build dependencies only (cached until Cargo.toml/lock changes)
RUN cargo build --release --bin riley-leaderboards 2>/dev/null || true

# Copy actual source and build for real
COPY crates/ crates/
RUN cargo build --release --bin riley-leaderboards

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/riley-leaderboards /usr/local/bin/
COPY --from=builder /app/crates/riley-leaderboards-core/migrations/ /app/migrations/
RUN mkdir -p /etc/riley_leaderboards && \
    printf '[database]\nurl = "env:DATABASE_URL"\n' > /etc/riley_leaderboards/config.toml
EXPOSE 8082
CMD ["riley-leaderboards", "serve"]
