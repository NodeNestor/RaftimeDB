# Build stage
FROM rust:bookworm AS builder

WORKDIR /app
COPY . .
RUN cargo build --release --bin rafttimedb

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/rafttimedb /usr/local/bin/rafttimedb

EXPOSE 3001 4001

ENTRYPOINT ["rafttimedb"]
