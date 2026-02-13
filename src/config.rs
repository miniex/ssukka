/// Configuration for HTML obfuscation.
///
/// All options default to `true` (maximum obfuscation).
/// Use the builder on [`crate::Obfuscator`] for ergonomic construction.
#[derive(Debug, Clone)]
pub struct ObfuscationConfig {
    // HTML
    pub remove_comments: bool,
    pub collapse_whitespace: bool,
    pub encode_text_entities: bool,
    pub encode_attr_entities: bool,
    pub shuffle_attributes: bool,
    pub randomize_tag_case: bool,

    // CSS
    pub rename_classes: bool,
    pub rename_ids: bool,
    pub minify_css: bool,
    pub unicode_escape_selectors: bool,

    // JS
    pub encode_js_strings: bool,
    pub minify_js: bool,

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

            rename_classes: true,
            rename_ids: true,
            minify_css: true,
            unicode_escape_selectors: true,

            encode_js_strings: true,
            minify_js: true,

            seed: None,
        }
    }
}
