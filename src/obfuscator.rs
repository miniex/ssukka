use crate::analysis;
use crate::config::ObfuscationConfig;
use crate::error::Result;
use crate::transform;

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

    /// Obfuscate HTML using a two-pass approach.
    ///
    /// Pass 1 collects all class/ID symbols, then Pass 2 applies all transformations.
    pub fn obfuscate(&self, html: &str) -> Result<String> {
        // Pass 1: Analyze and collect symbols
        let symbols = analysis::analyze(html, &self.config);

        // Pass 2: Transform with collected symbols
        transform::transform(html, &symbols, &self.config)
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

    pub fn encode_js_strings(mut self, v: bool) -> Self {
        self.config.encode_js_strings = v;
        self
    }

    pub fn minify_js(mut self, v: bool) -> Self {
        self.config.minify_js = v;
        self
    }

    pub fn seed(mut self, seed: u64) -> Self {
        self.config.seed = Some(seed);
        self
    }

    pub fn build(self) -> Obfuscator {
        Obfuscator {
            config: self.config,
        }
    }
}
