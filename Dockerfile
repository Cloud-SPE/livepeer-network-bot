FROM rust:1.95-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY migrations ./migrations
COPY build.rs* ./
COPY docs/generated/openapi.json ./docs/generated/openapi.json

RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/livepeer-payout-bot /usr/local/bin/livepeer-payout-bot

CMD ["livepeer-payout-bot"]
