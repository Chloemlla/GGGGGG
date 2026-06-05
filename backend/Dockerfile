FROM rust:1.89-bookworm AS builder

WORKDIR /app/backend

COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/src ./src

RUN cargo build --release

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/backend/target/release/pixel-api /app/pixel-api

ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080

ENTRYPOINT ["/app/pixel-api"]
