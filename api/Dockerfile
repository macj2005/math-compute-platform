FROM rust:1.91-slim AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src

RUN cargo build --release --bins

FROM debian:trixie-slim AS runtime

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/api /app/api
COPY --from=builder /app/target/release/worker /app/worker

EXPOSE 3000

CMD ["/app/api"]
