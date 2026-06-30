use crate::analysis;
use crate::config::{JsStringEncoding, ObfuscationConfig};
use crate::error::Result;
use crate::transform;
use std::path::PathBuf;

/// HTML obfuscator with builder pattern for configuration.
///
/// # Example
/// ```
/// let result = ssukka::Obfuscator::builder()
///     .rename_classes(true)
///     .seed(42)
///     .build()
///     .obfuscate("<div class=\"foo\">hello</div>")
///     .unwrap();
/// ```
#[derive(Default)]
pub struct Obfuscator {
    config: ObfuscationConfig,
}

impl Obfuscator {
    /// Create a new builder for configuring the obfuscator.
    pub fn builder() -> ObfuscatorBuilder {
        ObfuscatorBuilder {
            config: ObfuscationConfig::default(),
        }
    }

    /// Obfuscate HTML: collect every class/ID symbol, then apply the transforms.
    pub fn obfuscate(&self, html: &str) -> Result<String> {
        let config = self.effective_config();

        // Optional: inline local stylesheets/scripts so their content is
        // obfuscated like the rest. Local files only, never network.
        let inlined;
        let html: &str = if config.inline_local_resources {
            inlined = crate::inline::inline_local(html, config.base_dir.as_deref());
            &inlined
        } else {
            html
        };

        let symbols = analysis::analyze(html, &config);

        transform::transform(html, &symbols, &config)
    }

    /// Resolve the config for one invocation. In polymorphic mode (and only
    /// without a fixed `seed`), a random subset of *safe* cosmetic transforms
    /// is toggled/varied; correctness-critical ones are never touched.
    fn effective_config(&self) -> ObfuscationConfig {
        use rand::{rngs::StdRng, RngExt, SeedableRng};

        if !self.config.polymorphic || self.config.seed.is_some() {
            return self.config.clone();
        }

        let mut rng = StdRng::from_rng(&mut rand::rng());
        let mut c = self.config.clone();
        c.randomize_tag_case = rng.random_bool(0.7);
        c.shuffle_attributes = rng.random_bool(0.85);
        c.unicode_escape_selectors = rng.random_bool(0.7);
        c.collapse_whitespace = rng.random_bool(0.9);
        if c.inject_honeypots {
            let base = c.honeypot_count.max(2);
            c.honeypot_count = rng.random_range(2..=base + 6);
        }
        c
    }
}

/// Builder for [`Obfuscator`].
pub struct ObfuscatorBuilder {
    config: ObfuscationConfig,
}

impl ObfuscatorBuilder {
    pub fn remove_comments(mut self, v: bool) -> Self {
        self.config.remove_comments = v;
        self
    }

    pub fn collapse_whitespace(mut self, v: bool) -> Self {
        self.config.collapse_whitespace = v;
        self
    }

    pub fn encode_text_entities(mut self, v: bool) -> Self {
        self.config.encode_text_entities = v;
        self
    }

    pub fn encode_attr_entities(mut self, v: bool) -> Self {
        self.config.encode_attr_entities = v;
        self
    }

    pub fn shuffle_attributes(mut self, v: bool) -> Self {
        self.config.shuffle_attributes = v;
        self
    }

    pub fn randomize_tag_case(mut self, v: bool) -> Self {
        self.config.randomize_tag_case = v;
        self
    }

    pub fn split_words(mut self, v: bool) -> Self {
        self.config.split_words = v;
        self
    }

    pub fn rename_classes(mut self, v: bool) -> Self {
        self.config.rename_classes = v;
        self
    }

    pub fn rename_ids(mut self, v: bool) -> Self {
        self.config.rename_ids = v;
        self
    }

    pub fn minify_css(mut self, v: bool) -> Self {
        self.config.minify_css = v;
        self
    }

    pub fn unicode_escape_selectors(mut self, v: bool) -> Self {
        self.config.unicode_escape_selectors = v;
        self
    }

    /// Compat toggle: `true` maps to [`JsStringEncoding::Escapes`], `false` to
    /// [`JsStringEncoding::None`].
    pub fn encode_js_strings(mut self, v: bool) -> Self {
        self.config.js_string_encoding = if v {
            JsStringEncoding::Escapes
        } else {
            JsStringEncoding::None
        };
        self
    }

    /// Select the JS string-literal encoding strategy directly.
    pub fn js_string_encoding(mut self, e: JsStringEncoding) -> Self {
        self.config.js_string_encoding = e;
        self
    }

    pub fn minify_js(mut self, v: bool) -> Self {
        self.config.minify_js = v;
        self
    }

    pub fn inject_honeypots(mut self, v: bool) -> Self {
        self.config.inject_honeypots = v;
        self
    }

    pub fn honeypot_count(mut self, n: usize) -> Self {
        self.config.honeypot_count = n;
        self
    }

    pub fn structural_obfuscation(mut self, v: bool) -> Self {
        self.config.structural_obfuscation = v;
        self
    }

    pub fn js_ast(mut self, v: bool) -> Self {
        self.config.js_ast = v;
        self
    }

    pub fn mangle_identifiers(mut self, v: bool) -> Self {
        self.config.mangle_identifiers = v;
        self
    }

    pub fn poison_names(mut self, v: bool) -> Self {
        self.config.poison_names = v;
        self
    }

    pub fn control_flow_flattening(mut self, v: bool) -> Self {
        self.config.control_flow_flattening = v;
        self
    }

    pub fn dead_code_injection(mut self, v: bool) -> Self {
        self.config.dead_code_injection = v;
        self
    }

    pub fn dead_code_threshold(mut self, t: f32) -> Self {
        self.config.dead_code_threshold = t.clamp(0.0, 1.0);
        self
    }

    pub fn self_defending(mut self, v: bool) -> Self {
        self.config.self_defending = v;
        self
    }

    /// Replace integer literals with equivalent mixed boolean-arithmetic (AST).
    pub fn mba(mut self, v: bool) -> Self {
        self.config.mba = v;
        self
    }

    /// Embed `id` once as an invisible zero-width watermark in the text.
    pub fn watermark(mut self, id: u64) -> Self {
        self.config.watermark = Some(id);
        self
    }

    /// Inject standards-aligned `<meta>` AI opt-out signals (legacy `noai`,
    /// TDMRep, AIPREF). See [`crate::ai_opt_out`] for the non-HTML transports.
    pub fn emit_ai_opt_out(mut self, v: bool) -> Self {
        self.config.emit_ai_opt_out = v;
        self
    }

    pub fn inline_local_resources(mut self, v: bool) -> Self {
        self.config.inline_local_resources = v;
        self
    }

    pub fn base_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.config.base_dir = Some(dir.into());
        self
    }

    pub fn polymorphic(mut self, v: bool) -> Self {
        self.config.polymorphic = v;
        self
    }

    pub fn seed(mut self, seed: u64) -> Self {
        self.config.seed = Some(seed);
        self
    }

    pub fn build(self) -> Obfuscator {
        Obfuscator { config: self.config }
    }
}
