#!/bin/sh
set -eu

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
echo "Publishing ssukka v${VERSION}"

cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test

# Publish the engine first, then the facade/CLI that depends on it.
# (ssukka-proxy and ssukka_wasm are publish=false.)
cargo publish -p ssukka_core
cargo publish -p ssukka

echo "Published ssukka v${VERSION}"
