FROM rust:1.94-slim AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY tests/ tests/
COPY benches/ benches/

RUN cargo build --release --locked

FROM debian:bookworm-slim

COPY --from=builder /build/target/release/ssukka /usr/local/bin/ssukka

ENTRYPOINT ["ssukka"]
