#!/bin/sh
set -eu

cd "$(git rev-parse --show-toplevel)"

echo "formatting rust"
cargo fmt --all

echo "formatting shell"
find tools -name '*.sh' -type f -exec shfmt -w {} +

echo "formatting toml"
find . -name '*.toml' -type f ! -path './target/*' -exec taplo fmt {} +

echo "formatting markdown"
npx --yes prettier --write "**/*.md"
