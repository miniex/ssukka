# ssukka

> **쓰까(ssukka)** is Busan dialect (부산 사투리) for "mix it up" (섞어).

HTML obfuscation library and CLI for Rust. Renders identically in browsers but is hard for humans to read.

## Features

### Cosmetic (on by default)

- **Class/ID renaming** - consistent across HTML, CSS, and JavaScript (including dynamic construction patterns)
- **HTML entity encoding** - text and attribute values encoded as decimal/hex/named entities
- **Tag case randomization** - `<div>` becomes `<DiV>`
- **Attribute shuffling** - randomized attribute order
- **CSS minification** - via [lightningcss](https://github.com/parcel-bundler/lightningcss)
- **CSS selector unicode escaping** - `.foo` becomes `.\66\6f\6f`
- **JS string encoding** - string literals encoded with a randomized mix of `\xHH` / `\uXXXX` / `\u{..}` (strict-mode-safe)
- **JS minification** - comment removal and whitespace compression
- **Comment removal** and **whitespace collapsing**
- **Deterministic output** - seed-based RNG for reproducible results

### Advanced (opt-in)

These change the DOM, output size, runtime cost, or accessibility, so they are **off by default**:

- **Honeypots / decoys** (`--honeypots N`) - inject invisible trap links, fake form fields, and bogus data blocks to waste scraper effort and poison harvested data. Hidden from layout **and** assistive tech.
- **Structural obfuscation** (`--structural`) - move visible text out of the static markup into base64 `data-` attributes, restored client-side by an injected script. Resists static scrapers (curl / readability extractors that don't run JS) while rendering identically. Warning: breaks no-JS, SEO, and degrades accessibility.
- **AST JS engine** (`--js-ast`, powered by [oxc](https://github.com/oxc-project/oxc)):
  - **Identifier mangling** (`--mangle`) - scope-aware renaming of _local_ JS bindings (never globals, so cross-script / inline-handler references stay intact).
  - **String array** (`--js-string-encoding array`) - hoist string literals into a base64 array decoded at runtime.
  - **Dead code injection** (`--dead-code`) - opaque-predicate-guarded junk that never executes.
  - **Control-flow flattening** (`--cff`) - reshape sequential logic into a shuffled `switch` dispatcher.
- **Polymorphic mode** (`--polymorphic`) - vary which transforms run (and how) on every invocation, so identical input yields structurally different output each time (signature/cache evasion).
- **WASM build** - runs in the browser / Cloudflare Workers / Deno via the `wasm` feature.

Every AST transform re-parses its own output and is discarded if it would emit invalid JavaScript; on a parse failure the engine falls back to the token-based path. The transforms are semantics-preserving by construction (verified by executing obfuscated output under Node).

## Threat model

ssukka raises the **cost** of reading and scraping a page; it is **not a security boundary**. Anything the browser can render, a determined adversary with a headless browser can recover. Use it to deter casual copying and cheap bulk scraping, not to protect secrets.

Modern LLM-based deobfuscators can reverse simple identifier renaming and string encoding, so the strongest configurations **layer** transforms (string array + mangling + structural + honeypots) and lean on structural/visual approaches rather than renaming alone. Benchmark the presets with `cargo bench`.

## Offline by design

ssukka performs **no network I/O** at runtime - verified with `strace` (zero socket/connect syscalls). All dependencies (lol_html, lightningcss, oxc, rand) are pure Rust and compile from source. External resource inlining (`--inline-local-resources`) reads **local files only** and never fetches over the network.

## Architecture

Streaming pipeline built on [lol_html](https://github.com/cloudflare/lol-html):

```text
Input HTML
  -> Pass 0 (optional): inline local CSS/JS
  -> Pass 1: analyze -> SymbolMap
  -> Pass 2: transform HTML / CSS / JS
  -> Output HTML
```

## Installation

```bash
cargo install ssukka
```

Or build from source:

```bash
git clone https://github.com/miniex/ssukka.git
cd ssukka
cargo build --release
```

## CLI Usage

```bash
# Basic
ssukka -i input.html -o output.html

# stdin/stdout
cat input.html | ssukka > output.html

# With options
ssukka -i input.html -o output.html --seed 42 --no-rename --no-minify-css
```

### Options

| Flag | Description |
| --- | --- |
| `-i, --input <FILE>` | Input HTML file (default: stdin) |
| `-o, --output <FILE>` | Output file (default: stdout) |
| `--seed <N>` | Seed for deterministic output |
| `--no-rename` | Disable class/ID renaming |
| `--no-minify-css` | Disable CSS minification |
| `--no-minify-js` | Disable JS minification |
| `--no-encode-entities` | Disable entity encoding |
| `--no-shuffle-attrs` | Disable attribute shuffling |
| `--no-randomize-case` | Disable tag case randomization |
| `--js-string-encoding <none\|escapes\|array>` | JS string strategy (default: `escapes`) |
| `--honeypots <N>` | Inject N invisible decoy nodes (scraper traps) |
| `--structural` | Move text into encoded attrs, restore client-side |
| `--polymorphic` | Randomize transforms per run (ignored with `--seed`) |
| `--js-ast` | Use the oxc AST engine for `<script>` JS |
| `--mangle` | Scope-aware local identifier renaming (implies `--js-ast`) |
| `--cff` | Control-flow flattening (implies `--js-ast`) |
| `--dead-code` | Opaque-predicate dead code injection (implies `--js-ast`) |
| `--dead-code-threshold <0..1>` | Fraction of sites that receive dead code |
| `--inline-local-resources` | Inline local `<link>`/`<script src>` (offline only) |
| `--base-dir <DIR>` | Base directory for resolving local resources |

```bash
# Maximum: layered obfuscation for the strongest output
ssukka -i input.html -o output.html \
    --honeypots 8 --structural --mangle --cff --dead-code \
    --js-string-encoding array
```

## Library Usage

```rust
// Simple
let result = ssukka::obfuscate(html)?;

// With configuration
let result = ssukka::Obfuscator::builder()
    .seed(42)
    .rename_classes(true)
    .rename_ids(true)
    .encode_text_entities(true)
    .minify_css(true)
    .build()
    .obfuscate(html)?;

// Advanced, layered
use ssukka::config::JsStringEncoding;
let result = ssukka::Obfuscator::builder()
    .inject_honeypots(true)
    .honeypot_count(8)
    .structural_obfuscation(true)
    .js_ast(true)
    .mangle_identifiers(true)
    .js_string_encoding(JsStringEncoding::Array)
    .dead_code_injection(true)
    .control_flow_flattening(true)
    .build()
    .obfuscate(html)?;
```

## WASM

```bash
cargo build --release --target wasm32-unknown-unknown --features wasm
# or: wasm-pack build --features wasm
```

Exposes `obfuscate(html)`, `obfuscate_seeded(html, seed)`, and `obfuscate_max(html, honeypots, seed)`.

## Limitations

- The default JS path (no `--js-ast`) is token-based: string encoding + minification only. Enable `--js-ast` for AST-grade mangling / string arrays / dead code / control-flow flattening.
- Dynamic class/ID construction in JS is handled via prefix-detection heuristics on the token path; highly dynamic patterns may not be caught.
- AST control-flow flattening is conservative: it only flattens top-level sequences of simple expression statements (anything with declarations or control flow is left as-is to guarantee correctness).
- External stylesheets/scripts are only processed with `--inline-local-resources` and only from the local filesystem.
- Obfuscation is a deterrent, not security - see [Threat model](#threat-model).

## Development

Requires Rust >= 1.94 (pinned in `rust-toolchain.toml`). A `Dockerfile` and a Nix flake (`nix develop` / `nix build`) are also provided. The repo ships POSIX `sh` tooling:

```bash
./tools/format.sh   # cargo fmt + shfmt + taplo fmt + prettier (md)
./tools/lint.sh     # cargo clippy -D warnings + shellcheck + shfmt -d + taplo
```

## License

MIT
