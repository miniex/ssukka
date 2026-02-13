#!/usr/bin/env bash
set -euo pipefail

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
echo "Publishing ssukka v${VERSION}"

cargo fmt -- --check
cargo clippy -- -D warnings
cargo test

cargo publish

echo "Published ssukka v${VERSION}"
