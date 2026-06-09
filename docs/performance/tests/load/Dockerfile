# Stage 1 — build
FROM rust:slim AS builder

WORKDIR /app

# cache dependencies
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src

# build the real binary
COPY src ./src
RUN touch src/main.rs && cargo build --release

# Stage 2 — runtime
FROM debian:trixie-slim

RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/marreta ./marreta
COPY examples ./examples

EXPOSE 8080

CMD ["./marreta", "serve", "examples/ecommerce/app.marreta"]
