use std::path::PathBuf;

/// JavaScript string-literal encoding strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JsStringEncoding {
    /// Leave string literals untouched.
    None,
    /// Per-char escapes randomized over `\xHH` / `\uXXXX` / `\u{..}`
    /// (strict-mode-safe, token-level). Default.
    #[default]
    Escapes,
    /// Hoist string literals into a base64 array decoded by an injected
    /// runtime prelude. Needs the AST engine ([`ObfuscationConfig::js_ast`]);
    /// falls back to [`JsStringEncoding::Escapes`] when it is off.
    Array,
}

/// Configuration for HTML obfuscation.
///
/// Cosmetic transforms default to `true`. Transforms that change the DOM,
/// output size, runtime cost, or accessibility are opt-in (`false`). Build via
/// the [`crate::Obfuscator`] builder.
#[derive(Debug, Clone)]
pub struct ObfuscationConfig {
    // HTML (cosmetic, on by default)
    pub remove_comments: bool,
    pub collapse_whitespace: bool,
    pub encode_text_entities: bool,
    pub encode_attr_entities: bool,
    pub shuffle_attributes: bool,
    pub randomize_tag_case: bool,
    /// Insert empty comments inside long words so naive raw-HTML scrapers see
    /// fragmented text; browsers and content extractors read it intact. Opt-in.
    pub split_words: bool,

    // CSS (cosmetic, on by default)
    pub rename_classes: bool,
    pub rename_ids: bool,
    pub minify_css: bool,
    pub unicode_escape_selectors: bool,

    // JS (cosmetic, on by default)
    pub js_string_encoding: JsStringEncoding,
    pub minify_js: bool,
    /// String literals kept out of the string array (matched by exact value), so
    /// strings that reflection/`eval` must see stay readable.
    pub reserved_strings: Vec<String>,
    /// Fraction (0.0..=1.0) of eligible string literals the string array encodes;
    /// below 1.0 trades coverage for size/speed.
    pub string_array_threshold: f32,

    // Honeypots / decoys (opt-in)
    /// Inject invisible decoy links, fields, and classes to trap scrapers.
    pub inject_honeypots: bool,
    /// Number of decoy nodes to inject (when [`Self::inject_honeypots`]).
    pub honeypot_count: usize,

    // Structural obfuscation (opt-in, WebCloak-style)
    /// Move text content into encoded data-attributes and restore it
    /// client-side via an injected script. Resists static scrapers but
    /// requires JS execution and degrades no-JS / SEO / accessibility.
    pub structural_obfuscation: bool,

    // AST-based JS engine (opt-in, oxc)
    /// Route `<script>` JS through the oxc AST pipeline instead of the token
    /// state machine. Required by mangling / string arrays / CFF / dead code.
    pub js_ast: bool,
    /// Scope-aware renaming of local JS bindings (requires [`Self::js_ast`]).
    pub mangle_identifiers: bool,
    /// Rename local JS bindings to plausible-but-misleading names instead of
    /// short ones, to mislead LLM cleanup passes (requires [`Self::js_ast`]).
    pub poison_names: bool,
    /// Flatten sequential control flow into a switch dispatcher (requires AST).
    pub control_flow_flattening: bool,
    /// Inject opaque-predicate-guarded dead code (requires AST).
    pub dead_code_injection: bool,
    /// Fraction (0.0..=1.0) of eligible sites that receive dead code.
    pub dead_code_threshold: f32,
    /// Inject a self-check that disables `console` if the emitted script was
    /// beautified/tampered (deters casual beautify-and-run; requires AST).
    pub self_defending: bool,
    /// Replace integer literals with equivalent mixed boolean-arithmetic so an
    /// LLM/static pass can't trivially read the constant (requires AST).
    pub mba: bool,
    /// Wrap top-level expression statements in always-true opaque-predicate
    /// guards, putting real code behind a condition to analyze (requires AST).
    pub opaque_predicates: bool,
    /// Convert object-literal keys to computed string keys (`{foo:1}` ->
    /// `{["foo"]:1}`) so the string array can encode them. Requires AST.
    pub property_keys: bool,
    /// Allowed hostnames; if non-empty, inject a guard that crashes the script
    /// off these domains (and their subdomains). Requires AST.
    pub domain_lock: Vec<String>,
    /// Unix expiry in seconds; if set, inject a guard that crashes the script
    /// after this time. Requires AST.
    pub lock_expiry_secs: Option<u64>,

    // Watermark / provenance (opt-in)
    /// Embed this id once as invisible zero-width characters in the text, so a
    /// scraped/leaked copy can be traced. May affect screen readers.
    pub watermark: Option<u64>,

    // Machine-readable AI opt-out (opt-in)
    /// Inject standards-aligned `<meta>` opt-out signals into `<head>` (legacy
    /// `noai`, TDMRep `tdm-reservation`, AIPREF `Content-Usage`); the non-HTML
    /// transports live in [`crate::ai_opt_out`]. Legally recognized but widely
    /// ignored on its own.
    pub emit_ai_opt_out: bool,

    // External resources (opt-in, local files only, stays offline)
    /// Inline and obfuscate `<link rel=stylesheet>` / `<script src>` whose URL
    /// resolves to a **local file** under [`Self::base_dir`]. Never fetches
    /// over the network.
    pub inline_local_resources: bool,
    /// Base directory used to resolve local resource paths.
    pub base_dir: Option<PathBuf>,

    // Polymorphism / determinism
    /// Randomize *which* optional cosmetic transforms run and their intensity
    /// on each invocation, so identical input yields structurally different
    /// output every time (signature/cache evasion). Ignored when a `seed` is set.
    pub polymorphic: bool,

    /// Optional seed for deterministic output.
    pub seed: Option<u64>,
}

impl Default for ObfuscationConfig {
    fn default() -> Self {
        Self {
            remove_comments: true,
            collapse_whitespace: true,
            encode_text_entities: true,
            encode_attr_entities: true,
            shuffle_attributes: true,
            randomize_tag_case: true,
            split_words: false,

            rename_classes: true,
            rename_ids: true,
            minify_css: true,
            unicode_escape_selectors: true,

            js_string_encoding: JsStringEncoding::Escapes,
            minify_js: true,
            reserved_strings: Vec::new(),
            string_array_threshold: 1.0,

            inject_honeypots: false,
            honeypot_count: 6,

            structural_obfuscation: false,

            js_ast: false,
            mangle_identifiers: false,
            poison_names: false,
            control_flow_flattening: false,
            dead_code_injection: false,
            dead_code_threshold: 0.4,
            self_defending: false,
            mba: false,
            opaque_predicates: false,
            property_keys: false,
            domain_lock: Vec::new(),
            lock_expiry_secs: None,

            watermark: None,
            emit_ai_opt_out: false,

            inline_local_resources: false,
            base_dir: None,

            polymorphic: false,
            seed: None,
        }
    }
}

impl ObfuscationConfig {
    /// Whether any AST-only JS transform is requested.
    pub fn wants_ast(&self) -> bool {
        self.js_ast
            && (self.mangle_identifiers
                || self.poison_names
                || self.control_flow_flattening
                || self.dead_code_injection
                || self.self_defending
                || self.mba
                || self.opaque_predicates
                || self.property_keys
                || !self.domain_lock.is_empty()
                || self.lock_expiry_secs.is_some()
                || self.js_string_encoding == JsStringEncoding::Array)
    }
}
