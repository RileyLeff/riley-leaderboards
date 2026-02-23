FROM rust:1.88 AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY migrations/ migrations/
RUN cargo build --release --bin riley-leaderboards

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/riley-leaderboards /usr/local/bin/
COPY --from=builder /app/migrations/ /app/migrations/
RUN mkdir -p /etc/riley_leaderboards && \
    printf '[database]\nurl = "env:DATABASE_URL"\n' > /etc/riley_leaderboards/config.toml
EXPOSE 8082
CMD ["riley-leaderboards", "serve"]
