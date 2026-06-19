# Changelog

Based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-06-20

Opt-in advanced obfuscation layers, an AST-based JS engine, a WASM target, and POSIX dev tooling. Advanced transforms are off by default; cosmetic behavior is unchanged.

### Added

- **Honeypots** (`--honeypots N`): invisible decoy links/fields/data that trap scrapers, hidden from layout and assistive tech. (`src/honeypot.rs`)
- **Structural obfuscation** (`--structural`): moves text into base64 `data-ssk` attributes restored client-side; resists static scrapers. (`src/structural.rs`)
- **AST JS engine** (`--js-ast`, via [oxc](https://github.com/oxc-project/oxc), `src/js_ast.rs`): local identifier mangling (`--mangle`), string arrays (`--js-string-encoding array`), opaque-predicate dead code (`--dead-code`), and control-flow flattening (`--cff`). Each step re-parses its output and is dropped if invalid; parse failure falls back to the token path.
- **Polymorphic mode** (`--polymorphic`): varies transforms per run.
- **Local resource inlining** (`--inline-local-resources`, `--base-dir`): inlines local `<link>`/`<script src>`, then obfuscates them. Local files only, never network. (`src/inline.rs`)
- **WASM** (`wasm` feature, `src/wasm.rs`): `obfuscate`, `obfuscate_seeded`, `obfuscate_max` for browser/edge.
- **Benchmark** (`benches/obfuscation.rs`, criterion): `cargo bench`.
- **POSIX dev tooling**: `tools/format.sh` (also runs `prettier` on Markdown), `tools/lint.sh`, plus `rustfmt.toml`, `rust-toolchain.toml`, `.editorconfig`, `.shellcheckrc`, `.prettierrc.json`, `.cargo/config.toml`, and `[lints]` in `Cargo.toml`.
- New `ObfuscationConfig` fields / builder methods and the `JsStringEncoding` enum.

### Changed

- JS string encoding now randomizes over `\xHH` / `\uXXXX` / `\u{..}`.
- MSRV is **1.94** (oxc), pinned across `Cargo.toml`, `rust-toolchain.toml`, the `Dockerfile`, and the Nix flake.
- Dependencies bumped to latest: `lol_html` 3.0 (Settings builder API), `lightningcss` 1.0.0-alpha.71, `rand` 0.10.1.
- Nix devShell adds the wasm32 target and shfmt / shellcheck / taplo / nodejs.
- `tools/deploy.sh` converted to POSIX `sh`.
- README expanded (features, threat model, offline, WASM, development).

### Fixed

- Clippy `-D warnings` cleanups: `collapsible_match`, `unnecessary_sort_by`, `needless_character_iteration`.

### Dependencies

- Added `oxc` 0.136, `criterion` (dev), and optional `wasm-bindgen` / `getrandom` (the wasm feature enables `wasm_js` on both getrandom majors in the tree).

### Notes

- No network I/O at runtime, verified with `strace` even with all layers on.
- AST transforms are verified semantics-preserving by running output under Node.
- Obfuscation is a deterrent, not a security boundary.

## [0.1.0]

Initial release: class/ID renaming across HTML/CSS/JS, entity encoding, tag-case randomization, attribute shuffling, CSS minification + selector unicode escaping, JS string encoding + minification, comment removal, whitespace collapsing, and seed-based deterministic output. Two-pass streaming on lol_html + lightningcss.
