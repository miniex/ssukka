# syntax=docker/dockerfile:1
# Targets:
#   docker build -t ssukka .                              # CLI (default)
#   docker build --target ssukka-proxy -t ssukka-proxy .  # reverse proxy
#   docker build --target wasm -o type=local,dest=pkg .   # wasm package -> ./pkg

# Shared builder for the native binaries.
FROM rust:1.94-slim AS build
WORKDIR /build
COPY . .
RUN cargo build --release --locked -p ssukka -p ssukka-proxy

# WASM package builder (wasm-pack -> /pkg).
FROM rust:1.94-slim AS wasm-build
WORKDIR /build
RUN apt-get update \
 && apt-get install -y --no-install-recommends curl binaryen \
 && rm -rf /var/lib/apt/lists/* \
 && rustup target add wasm32-unknown-unknown \
 && curl -sSf https://rustwasm.github.io/wasm-pack/installer/init.sh | sh
COPY . .
RUN wasm-pack build wasm --release --target web --out-dir /pkg

# Extractable wasm artifact stage.
FROM scratch AS wasm
COPY --from=wasm-build /pkg /

# Reverse-proxy image.
FROM debian:bookworm-slim AS ssukka-proxy
COPY --from=build /build/target/release/ssukka-proxy /usr/local/bin/ssukka-proxy
EXPOSE 8080
ENTRYPOINT ["ssukka-proxy"]
CMD ["--listen", "0.0.0.0:8080"]

# CLI image (default, last stage).
FROM debian:bookworm-slim AS ssukka
COPY --from=build /build/target/release/ssukka /usr/local/bin/ssukka
ENTRYPOINT ["ssukka"]
