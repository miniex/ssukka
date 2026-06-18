#!/bin/sh
# No `set -e` - collect every tool's failure in one pass
# instead of bailing on the first.

cd "$(git rev-parse --show-toplevel)" || exit 1

log=$(mktemp)
trap 'rm -f "$log"' EXIT

failed=""

# Stay quiet on success - only surface output for the tools that actually broke.
run() {
	label=$1
	shift
	if ! "$@" >"$log" 2>&1; then
		echo "---- $label ----"
		cat "$log"
		echo
		failed="$failed $label"
	fi
}

run "rust" cargo clippy --all-targets -- -D warnings
run "shell" find tools -name '*.sh' -type f -exec shellcheck {} +
run "shfmt" find tools -name '*.sh' -type f -exec shfmt -d {} +
run "toml-format" find . -name '*.toml' -type f ! -path './target/*' -exec taplo fmt --check {} +
run "toml-lint" find . -name '*.toml' -type f ! -path './target/*' -exec taplo lint {} +

if [ -n "$failed" ]; then
	echo "lint failed:$failed"
	exit 1
fi
