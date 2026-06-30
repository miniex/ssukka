FROM rust:1.94-slim AS builder

WORKDIR /build
COPY . .

RUN cargo build --release --locked -p ssukka

FROM debian:bookworm-slim

COPY --from=builder /build/target/release/ssukka /usr/local/bin/ssukka

ENTRYPOINT ["ssukka"]
