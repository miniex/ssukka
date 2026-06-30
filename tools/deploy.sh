#!/bin/sh
set -eu

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
echo "Releasing ssukka v${VERSION}"

# Gates.
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test

# Sanity-build the wasm crate (host build is empty; this checks the wasm32 path).
cargo build --release -p ssukka-wasm --target wasm32-unknown-unknown

# Publish to crates.io in dependency order (cargo waits for the index between each).
cargo publish -p ssukka_core
cargo publish -p ssukka
cargo publish -p ssukka-proxy

# Also publish the wasm package to npm (opt-in: needs wasm-pack + `npm login`).
if [ "${NPM:-}" = "1" ] && command -v wasm-pack >/dev/null 2>&1; then
	wasm-pack build wasm --release --target web
	wasm-pack publish ssukka-wasm
else
	echo "note: set NPM=1 (with wasm-pack + npm login) to also publish ssukka-wasm to npm"
fi

echo "Released ssukka v${VERSION}"
