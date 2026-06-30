# Changelog

Based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Property-key transformation** (`--property-keys`, `property_keys(true)`): convert object-literal keys to computed string keys (`{foo: 1}` -> `{["foo"]: 1}`), so with the string array the key names are hoisted and encoded instead of left in plain sight. Skips methods, getters/setters, shorthand, already-computed keys, numeric keys, and `__proto__`. Implies `--js-ast`. (`src/js_ast.rs`)
- **String-array knobs** (`--reserved-strings`, `--string-array-threshold`; `reserved_strings(...)`, `string_array_threshold(...)`): a whitelist of strings to keep readable (for reflection/`eval`) and a fraction-encoded threshold to trade coverage for size/speed. (`src/js_ast.rs`)
- **OBsmith-style semantics tests** (`tests/obsmith.rs`): execute representative snippets under Node before and after the full JS pipeline and assert identical output, catching semantic deviations (not just parse failures); skipped when `node` is unavailable. (`tests/obsmith.rs`)
- **Execution lock** (`--domain-lock <hosts>`, `--lock-expiry <unix-secs>`; `domain_lock(...)`, `lock_expiry_secs(...)`): inject a guard that crashes the script (unbounded recursion -> uncaught `RangeError`) when run off an allowed host (or subdomain) or past an expiry, so the obfuscated code can't be lifted to another site or kept working forever. A no-op on the allowed domain before expiry; random identifiers avoid a fixed signature. A deterrent, not DRM (stripping the guard defeats it). Implies `--js-ast`. (`src/js_ast.rs`)
- **Opaque predicates** (`--opaque-predicates`, `opaque_predicates(true)`): wrap top-level expression statements in always-true guards (`if(<opaque>){ stmt }`) built from bitwise identities that hold for every non-negative int32 input (`a^b == (a|b)-(a&b)`, `a&b <= a|b`, `a|b >= a`), putting real code behind a condition a static/LLM pass must evaluate. Declarations and string-literal directives are left unwrapped. Implies `--js-ast`. (`src/js_ast.rs`)
- **MBA (mixed boolean-arithmetic)** (`--mba`, `mba(true)`): replace integer literals with equivalent bitwise/arithmetic expressions (`a+b == (a^b)+2*(a&b) == (a|b)+(a&b)`, exact for int32-range operands in JS), so a static or LLM pass must do the arithmetic to read the constant. Numeric property keys and out-of-range/float literals are left alone. Implies `--js-ast`. (`src/js_ast.rs`)
- **Readability-tier efficacy tests** (`tests/extraction.rs`, dev-dep `dom_smoothie` - a Mozilla Readability port): confirm `--structural` starves a real extractor (recall ~0) and the readability tier drops hidden decoys. A naive-tier test in `tests/efficacy.rs` shows `--structural` + `--honeypots` _poisons_ a visibility-blind scraper (real-body recall ~0.09 while it harvests decoy filler).
- **Editable decoy/word lists in `assets/`** via `ssukka::wordlist::parse` + `include_str!` (comma-separated, `#`-commented). Embedded at build time: editable without touching source, no runtime I/O (offline/WASM-safe). (`src/wordlist.rs`, `assets/honeypot/*.txt`)
- **AI opt-out transport helpers** (`ssukka::ai_opt_out`): standalone functions for the canonical AIPREF/TDMRep transports an HTML library can't emit itself — `robots_txt()` (a site-wide AIPREF `Content-Usage: train-ai=n` rule plus a `Disallow` group per known AI training crawler), `content_usage_header()` (the HTTP response-header value), and `well_known_tdmrep_json()` (the `/.well-known/tdmrep.json` body, with optional `tdm-policy` URL) — for edge/server (e.g. Cloudflare Worker) deployment. Targeted crawler tokens are exposed as `AI_TRAINING_CRAWLERS` and the tracked AIPREF vocabulary draft as `AIPREF_VOCAB_DRAFT`. (`src/ai_opt_out.rs`)

### Changed

- **Watermark is now redundant and error-correcting** (`--watermark`). The id is scattered as up to 8 copies across separate text nodes and recovered by majority vote over every complete frame, so it survives partial deletion (truncation, copying one section) and bit corruption - not just an intact copy-paste. Still pure zero-width (renders identically); a Unicode normalizer that strips the zero-width class still defeats it. (`src/watermark.rs`, `src/transform.rs`)
- **Honeypot decoy blocks now emit dense, link-free article-like prose** instead of short random tokens (`--honeypots`), wrapped in a content-like class (`article-body`, ...). An extractor starved of the real body by `--structural` scores the decoy as the article and harvests the filler (data poisoning, not just wasted effort). Still hidden, marked, and removed on load for JS clients. (`src/honeypot.rs`)
- **AI opt-out signals are now standards-aligned** (`--ai-opt-out`, `emit_ai_opt_out(true)`). The injected `<head>` `<meta>` block adds W3C **TDMRep** `tdm-reservation` (the EU CDSM Art.4 / AI Act rights-reservation lane) and a best-effort IETF **AIPREF** `Content-Usage: train-ai=n` (`http-equiv`) alongside the legacy `robots: noai, noimageai`. The block is centralized in the new `ssukka::ai_opt_out::meta_block` (with optional `tdm-policy` URL). (`src/ai_opt_out.rs`, `src/transform.rs`)

### Dependencies

- Bumped `oxc` 0.137 -> 0.138; MSRV stays 1.94 (oxc 0.138 still builds on it).

## [0.3.0] - 2026-06-29

### Added

- **Efficacy harness** (`cargo test --test efficacy`): a differential extraction test that runs a DOM-aware (trafilatura/BeautifulSoup-tier) text extractor before/after obfuscation and reports token recall, so resilience is measured rather than asserted. Gates that `--structural` starves non-JS extraction (recall ~0.06), that cosmetic transforms are friction only (recall ~1.0), and that `--comment-split` defeats substring search but not DOM extraction. Pure Rust, no external deps. (`tests/efficacy.rs`)
- **Poison names** (`--poison-names`, `poison_names(true)`): rename local JS bindings to plausible-but-misleading dictionary words (via the oxc mangler's debug slots, relabelled AST-safely) instead of short/base54 names, so an LLM "clean this up" pass anchors on names it keeps. Each slot gets a unique word that avoids every identifier already in the script, so no global/member/kept name is shadowed; verified semantics-preserving under Node. Implies `--js-ast`. (`src/js_ast.rs`)
- **Watermark** (`--watermark <N>`, `watermark(id)`): embed a 64-bit id once as invisible zero-width characters in the first eligible body text node, so a scraped/leaked copy can be traced. Renders invisibly and survives copy-paste; scoped to content (never `<title>`/metadata) and recoverable via `watermark::decode`. (`src/watermark.rs`, `src/transform.rs`)
- **AI opt-out signals** (`--ai-opt-out`, `emit_ai_opt_out(true)`): inject `<meta>` opt-out tags (`robots: noai, noimageai` and TDM reservation) into `<head>`. (`src/transform.rs`)
- **CLI warning channel**: aggressive options (`--structural`, `--watermark`, `--honeypots`) now print a stderr `warning:` naming the affected consumer (SEO/accessibility). (`src/main.rs`)
- **Word splitting** (`--comment-split`, `split_words(true)`): insert empty comments inside long words so naive regex/substring scrapers see fragmented text, while browsers, screen readers, find-in-page, and content extractors read it intact. Flow content only (never `<title>`/RCDATA); interleaves entity encoding so a marker never lands inside an entity. (`src/word_split.rs`, `src/transform.rs`)
- **Self-defending** (`--self-defending`, `self_defending(true)`): inject a canary whose `toString()` is hashed at load and compared to the build-time hash; if the script was beautified/tampered, `console` is stubbed out. Verified under Node (clean output runs, beautified output trips the guard). Deters casual beautify-and-run; a deobfuscator that strips the guard defeats it. Implies `--js-ast`. (`src/js_ast.rs`)
- **Keyframe / animation renaming**: `@keyframes` names and their `animation` / `animation-name` references are now obfuscated consistently (via a lightningcss visitor, so the shorthand and vendor prefixes are handled). Only locally-defined keyframes are renamed; external references are left intact. Grouped with class renaming. (`src/css.rs`, `src/symbol_map.rs`)

### Fixed

- **Whitespace collapse no longer doubles spaces at text-node splits.** lol_html may split one text node into chunks; collapsing each independently could emit two spaces where a whitespace run straddled the split ([lol-html#255](https://github.com/cloudflare/lol-html/issues/255)). General text is now buffered and collapsed once. (`src/transform.rs`)
- **IE conditional comments are preserved.** `remove_comments` also stripped `<!--[if ...]>...<![endif]-->`, which can change downlevel rendering; such comments are now kept. (`src/transform.rs`)
- **SVG/MathML names are no longer corrupted.** Inside `<svg>`/`<math>`, tag-case randomization and attribute encoding/reordering are skipped — each rewrites names through lol_html's lowercased `name()`, breaking case-sensitive names like SVG `viewBox`/`linearGradient`. (`src/transform.rs`)

### Changed

- **Whitespace-only text inside table/select containers is dropped.** Direct whitespace-only children of `table`/`thead`/`tbody`/`tfoot`/`tr`/`colgroup`/`select`/`optgroup`/`datalist` never render (the parser foster-parents or ignores them), so they are removed outright. List containers (`ul`/`ol`/`dl`) are deliberately excluded - an inline-block child could turn inter-item whitespace into a visible gap - and the more aggressive minify-html rules (trim block edges, destroy in any layout element) are not adopted because a streaming pass has no sibling lookahead to apply them safely. (`src/html/whitespace.rs`, `src/transform.rs`)
- **Dead-code and string-array shapes vary per build (anti-signature).** The opaque predicate now picks among several provably-always-false forms, the junk body among several shapes, and the string-array decoder among an index loop / `map` / `reduce`. Every form is verified semantics-preserving under Node, so no single structural signature identifies the output. (`src/js_ast.rs`)
- **String-array encoding is anti-hook and polymorphic.** String literals are hoisted into a per-build shuffled character pool decoded by offset-shifted index, instead of a base64 array decoded via `atob`/`TextDecoder`. This removes the standard decode primitives that hook-based deobfuscators (de4js, synchrony, restringer) target and de-recognizes the prelude. A tool that _executes_ the decoder still recovers the strings (the documented threat boundary). Verified semantics-preserving (incl. multibyte UTF-8) under Node. (`src/js_ast.rs`)
- **JS class/ID replacement is a single linear pass.** The per-name `replace_word` loops are replaced by one leftmost-longest `aho-corasick` automaton per namespace plus the same boundary check, so the result is identical but no longer O(names x text). (`src/js.rs`)
- **Honeypots are stripped on load instead of lingering in the DOM.** Each decoy now carries a random marker attribute and an injected script removes them on load, so JS clients (real users, JS-rendering scrapers) end up with a clean DOM and no honeypot signature to fingerprint, while no-JS bulk crawlers still ingest the decoys from the raw HTML. (`src/honeypot.rs`, `src/transform.rs`)
- **Structural obfuscation encoding is now polymorphic.** The fixed `data-ssk` + plain-base64 scheme (a published, generically-decodable signature) is replaced by a per-document `Scheme`: a random `data-*` attribute name, a cyclic XOR key, and an optional byte reverse, all baked into the matching restore script. No single static decoder recipe works across builds; verified semantics-preserving (incl. multibyte UTF-8) under Node. (`src/structural.rs`, `src/transform.rs`)
- **Attribute reordering is now gzip/brotli-friendly.** The per-element random shuffle is replaced by a document-stable order (FNV-1a of the name salted per document; deterministic under `--seed`). Output still differs from source, but identical tag shapes serialize identically. Seeded output bytes differ from 0.2.1. (`src/html/tags.rs`, `src/transform.rs`)

### Dependencies

- Bumped `oxc` 0.136 -> 0.137 and `wasm-bindgen` 0.2.125 -> 0.2.126; MSRV stays 1.94 (oxc 0.137 still builds on it).
- The Nix flake reads its package version from `Cargo.toml` instead of hardcoding it, so it can't go stale (verified with `nix build`). (`flake.nix`)

## [0.2.1] - 2026-06-20

### Fixed

- CSS class/ID renaming and selector unicode-escaping now operate **only on selector preludes**, never on declaration values, strings, or comments. Previously the renamer did a global substring replace, so a class/ID whose name collided with a hex color (e.g. an ID `abc` and `color:#abc`) or appeared inside a value string (`content:".x"`) could corrupt that value. Hex colors (`#fff`, `#abcdef`), quoted values, and `url(...)` tokens are now left intact. (`src/css.rs`)

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
