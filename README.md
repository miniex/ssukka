# ssukka

> **쓰까(ssukka)** is Busan dialect (부산 사투리) for "mix it up" (섞어).

HTML obfuscation library and CLI for Rust. Renders identically in browsers but is hard for humans to read.

## Features

### Cosmetic (on by default)

- **Class/ID/keyframe renaming** - consistent across HTML, CSS, and JavaScript (including dynamic construction patterns); `@keyframes` and their `animation` / `animation-name` references are renamed too
- **HTML entity encoding** - text and attribute values encoded as decimal/hex/named entities
- **Tag case randomization** - `<div>` becomes `<DiV>` (skipped inside `<svg>`/`<math>`, whose names are case-sensitive)
- **Attribute reordering** - document-stable, seed-derived order: differs from source yet keeps output gzip/brotli-friendly
- **CSS minification** - via [lightningcss](https://github.com/parcel-bundler/lightningcss)
- **CSS selector unicode escaping** - `.foo` becomes `.\66\6f\6f`
- **JS string encoding** - string literals encoded with a randomized mix of `\xHH` / `\uXXXX` / `\u{..}` (strict-mode-safe)
- **JS minification** - comment removal and whitespace compression
- **Comment removal** and **whitespace collapsing** - IE conditional comments are preserved; whitespace-only text inside table/select containers (where it never renders) is dropped, everything else collapses to a single space
- **Deterministic output** - seed-based RNG for reproducible results

### Advanced (opt-in)

These change the DOM, output size, runtime cost, or accessibility, so they are **off by default**:

- **Honeypots / decoys** (`--honeypots N`) - inject invisible trap links, fake form fields, and **decoy article-like prose blocks** (dense, link-free filler in content-like class names, so an extractor starved of the real body by `--structural` harvests the decoy instead - data poisoning). Hidden from layout **and** assistive tech, and **removed on load by an injected script**, so no-JS bulk crawlers take the bait while JS clients get a clean DOM. Vocabulary: editable `assets/honeypot/*.txt`.
- **Structural obfuscation** (`--structural`) - move visible text out of the static markup into a `data-` attribute, restored client-side by an injected script. The encoding is **polymorphic** (per-build random attribute name + XOR key + byte order), so no single static decoder recipe works across builds. Resists static scrapers (curl / readability extractors that don't run JS) while rendering identically. Warning: breaks no-JS, SEO, and degrades accessibility.
- **AST JS engine** (`--js-ast`, powered by [oxc](https://github.com/oxc-project/oxc)):
  - **Identifier mangling** (`--mangle`) - scope-aware renaming of _local_ JS bindings (never globals, so cross-script / inline-handler references stay intact).
  - **Poison names** (`--poison-names`) - rename _local_ bindings to plausible-but-misleading words (`cursor`, `vertex`, ...) instead of short ones, so an LLM "clean this up" pass anchors on names it keeps rather than re-deriving the originals. Each name is unique and avoids every identifier already in the script, so nothing is shadowed.
  - **String array** (`--js-string-encoding array`) - hoist string literals into a per-build shuffled character pool, decoded by (offset-shifted) index at runtime. Uses no `atob` / `String.fromCharCode` / `TextDecoder`, so hook-based deobfuscators have nothing to latch onto (a tool that _executes_ the decoder still recovers the strings).
  - **Dead code injection** (`--dead-code`) - opaque-predicate-guarded junk that never executes; predicate and body shapes vary per build so they aren't a fixed signature.
  - **Control-flow flattening** (`--cff`) - reshape sequential logic into a shuffled `switch` dispatcher.
  - **MBA (mixed boolean-arithmetic)** (`--mba`) - replace integer literals with equivalent bitwise/arithmetic expressions (`5` becomes a `(3^6)`-style form), so a static or LLM cleanup pass has to do the arithmetic to read the constant. Exact under JS int32 semantics.
  - **Opaque predicates** (`--opaque-predicates`) - wrap top-level expression statements in always-true guards (`if(<opaque>){ stmt }`) built from bitwise identities that hold for every input, so real code sits behind a condition the analyzer must evaluate. Skips declarations and directives.
  - **Execution lock** (`--domain-lock <hosts>`, `--lock-expiry <unix-secs>`) - inject a guard that crashes the script (unbounded recursion) when run off an allowed host (or its subdomains) or past an expiry, so the obfuscated code can't be lifted to another site or kept working forever. A no-op on the allowed domain before expiry; a deterrent, not DRM (an attacker who strips the guard defeats it).
  - **Self-defending** (`--self-defending`) - inject a check that disables `console` if the script was beautified or tampered with (deters casual beautify-and-run; a deobfuscator that strips the guard defeats it).
- **Polymorphic mode** (`--polymorphic`) - vary which transforms run (and how) on every invocation, so identical input yields structurally different output each time (signature/cache evasion).
- **Watermark** (`--watermark <N>`) - embed a build/recipient id as invisible zero-width characters in the body text, so a scraped or leaked copy can be traced. The id is scattered as redundant copies across multiple text nodes and recovered by majority vote, so it survives copy-paste and partial deletion (truncation, copying one section). Renders invisibly; may affect screen readers. Reversed only by a Unicode normalizer that strips the zero-width class.
- **AI opt-out signals** (`--ai-opt-out`) - inject standards-aligned `<meta>` opt-out tags into `<head>`: legacy `robots: noai`, W3C **TDMRep** (`tdm-reservation`, the EU CDSM Art.4 / AI Act rights-reservation lane), and a best-effort IETF **AIPREF** `Content-Usage: train-ai=n` (`http-equiv`). The canonical AIPREF/TDMRep transports - an HTTP `Content-Usage` response header, a `robots.txt` rule, and `/.well-known/tdmrep.json` - are exposed as library helpers (`ssukka::ai_opt_out::{meta_block, robots_txt, content_usage_header, well_known_tdmrep_json}`) for edge/server deployment. A legally recognized signal that bulk crawlers widely ignore on its own; pair it with the in-content deterrents above.
- **Word splitting** (`--comment-split`) - insert empty comments inside long words so naive regex/substring scrapers see fragmented text, while browsers, screen readers, find-in-page, and content extractors read it intact. Flow content only (never `<title>` or other RCDATA).
- **WASM build** - runs in the browser / Cloudflare Workers / Deno via the `wasm` feature.

Every AST transform re-parses its own output and is discarded if it would emit invalid JavaScript; on a parse failure the engine falls back to the token-based path. The transforms are semantics-preserving by construction (verified by executing obfuscated output under Node).

## Threat model

ssukka raises the **cost** of reading and scraping a page; it is **not a security boundary**. Anything the browser can render, a determined adversary with a headless browser can recover. Use it to deter casual copying and cheap bulk scraping, not to protect secrets.

Modern LLM-based deobfuscators can reverse simple identifier renaming and string encoding, so the strongest configurations **layer** transforms (string array + mangling + structural + honeypots) and lean on structural/visual approaches rather than renaming alone. Where renaming is used, `--poison-names` turns it from a no-op (an LLM just re-derives meaningful names) into a trap that anchors the cleanup pass on misleading names. Benchmark the presets with `cargo bench`, and measure extraction resistance with two differential harnesses: `cargo test --test efficacy` (naive/visibility-blind tier) and `cargo test --test extraction` (readability tier via the `dom_smoothie` dev-dependency). `--structural` drops recall to ~0 against both, cosmetic transforms are friction only (~1.0), and `--structural` + `--honeypots` poisons the naive tier - it harvests decoy filler instead of the body, while the readability tier drops the hidden decoys.

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
| `--no-shuffle-attrs` | Disable attribute reordering |
| `--no-randomize-case` | Disable tag case randomization |
| `--comment-split` | Split long words with empty comments (anti-regex-scraper) |
| `--js-string-encoding <none\|escapes\|array>` | JS string strategy (default: `escapes`) |
| `--honeypots <N>` | Inject N invisible decoy nodes (scraper traps) |
| `--structural` | Move text into encoded attrs, restore client-side |
| `--polymorphic` | Randomize transforms per run (ignored with `--seed`) |
| `--js-ast` | Use the oxc AST engine for `<script>` JS |
| `--mangle` | Scope-aware local identifier renaming (implies `--js-ast`) |
| `--poison-names` | Rename locals to misleading names (implies `--js-ast`) |
| `--cff` | Control-flow flattening (implies `--js-ast`) |
| `--mba` | Encode integer literals as mixed boolean-arithmetic (implies `--js-ast`) |
| `--opaque-predicates` | Wrap statements in always-true opaque guards (implies `--js-ast`) |
| `--domain-lock <HOSTS>` | Crash the script off these comma-separated hosts (implies `--js-ast`) |
| `--lock-expiry <UNIX_SECS>` | Crash the script after this Unix time (implies `--js-ast`) |
| `--dead-code` | Opaque-predicate dead code injection (implies `--js-ast`) |
| `--self-defending` | Disable `console` if the script is beautified (implies `--js-ast`) |
| `--dead-code-threshold <0..1>` | Fraction of sites that receive dead code |
| `--watermark <N>` | Embed an invisible zero-width id for provenance |
| `--ai-opt-out` | Inject AI opt-out `<meta>` (noai + TDMRep + AIPREF) into `<head>` |
| `--inline-local-resources` | Inline local `<link>`/`<script src>` (offline only) |
| `--base-dir <DIR>` | Base directory for resolving local resources |

```bash
# Maximum: layered obfuscation for the strongest output
ssukka -i input.html -o output.html \
    --honeypots 8 --structural --mangle --cff --dead-code --mba --opaque-predicates \
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
    .mba(true)
    .opaque_predicates(true)
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

## SEO, accessibility, and legal

ssukka renders identically for every client and never branches on user-agent or IP, so it is **not cloaking** - unlike tools that serve different content to bots. What it does and does not stop:

- **Stops** no-JS bulk fetchers and casual copy/paste. Most large AI-training crawlers (GPTBot, ClaudeBot, CCBot, and similar) do not execute JavaScript, so `--structural` content stays empty for them.
- **Does not stop** any client that renders JavaScript: headless browsers, on-demand "read this URL" agents, and Google/Gemini all run the restore script and recover the text.

When using the aggressive layers, keep these costs in mind:

- **SEO** - `--structural` moves text behind JS. Search engines that render JS (Google) usually recover it on a delayed pass, but unreliable or non-rendering crawlers index an empty shell. Keep SEO-critical copy in the static markup or provide a `<noscript>` fallback.
- **Accessibility** - `--structural` text is absent from the accessibility tree until the restore script runs; `--watermark` adds zero-width characters that some screen readers announce and that break programmatic text matching. Never apply these to content that must be reliably read by assistive tech. The CLI prints a stderr `warning:` for each aggressive option.
- **Legal** - `--ai-opt-out` emits the machine-readable signals (legacy robots `noai`, W3C TDMRep `tdm-reservation`, IETF AIPREF `Content-Usage`) that are the rising, legally-backed opt-out lever - TDMRep is the protocol EU rightsholders point to under the CDSM Directive Art.4 and the AI Act. They are widely ignored on their own, so treat them as complementary to the in-content deterrents, not a replacement. For the HTTP-header / robots.txt / `.well-known` transports, use the `ssukka::ai_opt_out` helpers at the edge.

## Development

Requires Rust >= 1.94 (pinned in `rust-toolchain.toml`). A `Dockerfile` and a Nix flake (`nix develop` / `nix build`) are also provided. The repo ships POSIX `sh` tooling:

```bash
./tools/format.sh   # cargo fmt + shfmt + taplo fmt + prettier (md)
./tools/lint.sh     # cargo clippy -D warnings + shellcheck + shfmt -d + taplo
```

Decoy/word vocabulary lives in `assets/honeypot/*.txt` (comma-separated, `#`-commented), embedded at build time via `include_str!` - edit a list and rebuild, no Rust changes and no runtime I/O.

## License

MIT
