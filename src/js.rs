use crate::config::JsStringEncoding;
use crate::symbol_map::SymbolMap;
use rand::rngs::StdRng;
use rand::RngExt;

/// State machine states for JS lexing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Normal,
    SingleLineComment,
    MultiLineComment,
    SingleQuoteString,
    DoubleQuoteString,
    TemplateString,
}

/// Transform JavaScript source: encode string literals, replace class/ID references, minify.
///
/// `encoding` selects the string-literal strategy. [`JsStringEncoding::Array`] is
/// handled by the AST engine; here it degrades to [`JsStringEncoding::Escapes`].
pub fn transform_js(
    js: &str,
    symbols: &SymbolMap,
    encoding: JsStringEncoding,
    minify: bool,
    rename_classes: bool,
    rename_ids: bool,
    rng: &mut StdRng,
) -> String {
    let mut result = js.to_owned();

    if rename_classes || rename_ids {
        result = replace_symbol_references(&result, symbols, rename_classes, rename_ids);
    }

    // `Array` requires the AST engine; without it we fall back to escapes.
    if encoding != JsStringEncoding::None {
        result = encode_js_strings(&result, rng);
    }

    if minify {
        result = minify_js(&result);
    }

    result
}

/// Pick a randomized escape form for a single character.
///
/// Only strict-mode-safe forms are emitted (`\xHH`, `\uXXXX`, `\u{..}`); octal
/// escapes are deliberately avoided since they throw in strict mode / templates.
fn escape_char(ch: char, out: &mut String, rng: &mut StdRng) {
    let code = ch as u32;
    if code <= 0xFF {
        match rng.random_range(0u8..3) {
            0 => out.push_str(&format!("\\x{code:02x}")),
            1 => out.push_str(&format!("\\u{code:04x}")),
            _ => out.push_str(&format!("\\u{{{code:x}}}")),
        }
    } else if code <= 0xFFFF {
        if rng.random_bool(0.5) {
            out.push_str(&format!("\\u{code:04x}"));
        } else {
            out.push_str(&format!("\\u{{{code:x}}}"));
        }
    } else if rng.random_bool(0.5) {
        out.push_str(&format!("\\u{{{code:x}}}"));
    } else {
        // Surrogate pair for characters above the BMP
        let hi = ((code - 0x10000) >> 10) + 0xD800;
        let lo = ((code - 0x10000) & 0x3FF) + 0xDC00;
        out.push_str(&format!("\\u{hi:04x}\\u{lo:04x}"));
    }
}

/// Replace class/ID names inside JS string literals.
///
/// Scans for patterns like `"foo"`, `'foo'`, `` `foo` `` and replaces
/// class/ID names found within them.
pub(crate) fn replace_symbol_references(
    js: &str,
    symbols: &SymbolMap,
    rename_classes: bool,
    rename_ids: bool,
) -> String {
    let chars: Vec<char> = js.chars().collect();
    let len = chars.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;
    let mut state = State::Normal;

    while i < len {
        match state {
            State::Normal => {
                if i + 1 < len && chars[i] == '/' && chars[i + 1] == '/' {
                    out.push(chars[i]);
                    state = State::SingleLineComment;
                    i += 1;
                } else if i + 1 < len && chars[i] == '/' && chars[i + 1] == '*' {
                    out.push(chars[i]);
                    state = State::MultiLineComment;
                    i += 1;
                } else if chars[i] == '\'' {
                    out.push(chars[i]);
                    state = State::SingleQuoteString;
                    i += 1;
                } else if chars[i] == '"' {
                    out.push(chars[i]);
                    state = State::DoubleQuoteString;
                    i += 1;
                } else if chars[i] == '`' {
                    out.push(chars[i]);
                    state = State::TemplateString;
                    i += 1;
                } else {
                    out.push(chars[i]);
                    i += 1;
                }
            },
            State::SingleLineComment => {
                out.push(chars[i]);
                if chars[i] == '\n' {
                    state = State::Normal;
                }
                i += 1;
            },
            State::MultiLineComment => {
                out.push(chars[i]);
                if chars[i] == '*' && i + 1 < len && chars[i + 1] == '/' {
                    out.push(chars[i + 1]);
                    i += 2;
                    state = State::Normal;
                } else {
                    i += 1;
                }
            },
            State::SingleQuoteString | State::DoubleQuoteString | State::TemplateString => {
                let quote = match state {
                    State::SingleQuoteString => '\'',
                    State::DoubleQuoteString => '"',
                    State::TemplateString => '`',
                    _ => unreachable!(),
                };

                let mut string_content = String::new();
                while i < len {
                    if chars[i] == '\\' && i + 1 < len {
                        let next = chars[i + 1];
                        string_content.push('\\');
                        string_content.push(next);
                        i += 2;
                        if next == 'u' {
                            if i < len && chars[i] == '{' {
                                while i < len {
                                    string_content.push(chars[i]);
                                    if chars[i] == '}' {
                                        i += 1;
                                        break;
                                    }
                                    i += 1;
                                }
                            } else {
                                for _ in 0..4 {
                                    if i < len {
                                        string_content.push(chars[i]);
                                        i += 1;
                                    }
                                }
                            }
                        } else if next == 'x' {
                            for _ in 0..2 {
                                if i < len {
                                    string_content.push(chars[i]);
                                    i += 1;
                                }
                            }
                        }
                    } else if chars[i] == quote {
                        break;
                    } else {
                        string_content.push(chars[i]);
                        i += 1;
                    }
                }

                // Replace symbols in the string content (word-boundary aware)
                let mut replaced = string_content;
                if rename_classes {
                    let mut class_pairs: Vec<_> = symbols.classes().iter().collect();
                    class_pairs.sort_by_key(|b| std::cmp::Reverse(b.0.len()));
                    for (original, obfuscated) in &class_pairs {
                        replaced = replace_word(&replaced, original, obfuscated);
                    }
                }
                if rename_ids {
                    let mut id_pairs: Vec<_> = symbols.ids().iter().collect();
                    id_pairs.sort_by_key(|b| std::cmp::Reverse(b.0.len()));
                    for (original, obfuscated) in &id_pairs {
                        replaced = replace_word(&replaced, original, obfuscated);
                    }
                }

                out.push_str(&replaced);

                if i < len {
                    out.push(chars[i]);
                    i += 1;
                }
                state = State::Normal;
            },
        }
    }

    out
}

/// Encode JS string literals with a randomized mix of escape forms.
fn encode_js_strings(js: &str, rng: &mut StdRng) -> String {
    let chars: Vec<char> = js.chars().collect();
    let len = chars.len();
    let mut out = String::with_capacity(len * 2);
    let mut i = 0;
    let mut state = State::Normal;

    while i < len {
        match state {
            State::Normal => {
                if i + 1 < len && chars[i] == '/' && chars[i + 1] == '/' {
                    out.push(chars[i]);
                    state = State::SingleLineComment;
                    i += 1;
                } else if i + 1 < len && chars[i] == '/' && chars[i + 1] == '*' {
                    out.push(chars[i]);
                    state = State::MultiLineComment;
                    i += 1;
                } else if chars[i] == '\'' {
                    out.push(chars[i]);
                    state = State::SingleQuoteString;
                    i += 1;
                } else if chars[i] == '"' {
                    out.push(chars[i]);
                    state = State::DoubleQuoteString;
                    i += 1;
                } else if chars[i] == '`' {
                    out.push(chars[i]);
                    state = State::TemplateString;
                    i += 1;
                } else {
                    out.push(chars[i]);
                    i += 1;
                }
            },
            State::SingleLineComment => {
                out.push(chars[i]);
                if chars[i] == '\n' {
                    state = State::Normal;
                }
                i += 1;
            },
            State::MultiLineComment => {
                out.push(chars[i]);
                if chars[i] == '*' && i + 1 < len && chars[i + 1] == '/' {
                    out.push(chars[i + 1]);
                    i += 2;
                    state = State::Normal;
                } else {
                    i += 1;
                }
            },
            State::SingleQuoteString | State::DoubleQuoteString | State::TemplateString => {
                let quote = match state {
                    State::SingleQuoteString => '\'',
                    State::DoubleQuoteString => '"',
                    State::TemplateString => '`',
                    _ => unreachable!(),
                };

                while i < len {
                    if chars[i] == '\\' && i + 1 < len {
                        // Keep existing escape sequences intact
                        let next = chars[i + 1];
                        out.push('\\');
                        out.push(next);
                        i += 2;
                        if next == 'u' {
                            if i < len && chars[i] == '{' {
                                // \u{...} code point escape
                                while i < len {
                                    out.push(chars[i]);
                                    if chars[i] == '}' {
                                        i += 1;
                                        break;
                                    }
                                    i += 1;
                                }
                            } else {
                                // \uXXXX - consume 4 hex digits
                                for _ in 0..4 {
                                    if i < len {
                                        out.push(chars[i]);
                                        i += 1;
                                    }
                                }
                            }
                        } else if next == 'x' {
                            // \xHH - consume 2 hex digits
                            for _ in 0..2 {
                                if i < len {
                                    out.push(chars[i]);
                                    i += 1;
                                }
                            }
                        }
                    } else if chars[i] == quote {
                        out.push(chars[i]);
                        i += 1;
                        state = State::Normal;
                        break;
                    } else if state == State::TemplateString && chars[i] == '$' && i + 1 < len && chars[i + 1] == '{' {
                        // Don't encode template literal expressions
                        out.push(chars[i]);
                        i += 1;
                    } else {
                        escape_char(chars[i], &mut out, rng);
                        i += 1;
                    }
                }
            },
        }
    }

    out
}

/// Basic JS minification: remove comments, collapse whitespace.
///
/// This is intentionally simple - we don't parse the full JS AST.
fn minify_js(js: &str) -> String {
    let chars: Vec<char> = js.chars().collect();
    let len = chars.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;
    let mut state = State::Normal;
    let mut prev_was_space = false;
    let mut prev_char: Option<char> = None;

    while i < len {
        match state {
            State::Normal => {
                if i + 1 < len && chars[i] == '/' && chars[i + 1] == '/' {
                    state = State::SingleLineComment;
                    i += 2;
                    continue;
                } else if i + 1 < len && chars[i] == '/' && chars[i + 1] == '*' {
                    state = State::MultiLineComment;
                    i += 2;
                    continue;
                } else if chars[i] == '\'' {
                    prev_was_space = false;
                    out.push(chars[i]);
                    state = State::SingleQuoteString;
                    prev_char = Some(chars[i]);
                    i += 1;
                } else if chars[i] == '"' {
                    prev_was_space = false;
                    out.push(chars[i]);
                    state = State::DoubleQuoteString;
                    prev_char = Some(chars[i]);
                    i += 1;
                } else if chars[i] == '`' {
                    prev_was_space = false;
                    out.push(chars[i]);
                    state = State::TemplateString;
                    prev_char = Some(chars[i]);
                    i += 1;
                } else if chars[i].is_ascii_whitespace() {
                    // Collapse whitespace but keep one space between identifiers/keywords
                    if !prev_was_space && needs_space_separator(prev_char, chars.get(i + 1).copied()) {
                        out.push(' ');
                    }
                    prev_was_space = true;
                    i += 1;
                } else {
                    prev_was_space = false;
                    out.push(chars[i]);
                    prev_char = Some(chars[i]);
                    i += 1;
                }
            },
            State::SingleLineComment => {
                if chars[i] == '\n' {
                    state = State::Normal;
                }
                i += 1;
            },
            State::MultiLineComment => {
                if chars[i] == '*' && i + 1 < len && chars[i + 1] == '/' {
                    i += 2;
                    state = State::Normal;
                } else {
                    i += 1;
                }
            },
            State::SingleQuoteString | State::DoubleQuoteString | State::TemplateString => {
                let quote = match state {
                    State::SingleQuoteString => '\'',
                    State::DoubleQuoteString => '"',
                    State::TemplateString => '`',
                    _ => unreachable!(),
                };
                out.push(chars[i]);
                if chars[i] == '\\' && i + 1 < len {
                    let next = chars[i + 1];
                    out.push(next);
                    prev_char = Some(next);
                    i += 2;
                    if next == 'u' {
                        if i < len && chars[i] == '{' {
                            while i < len {
                                out.push(chars[i]);
                                prev_char = Some(chars[i]);
                                if chars[i] == '}' {
                                    i += 1;
                                    break;
                                }
                                i += 1;
                            }
                        } else {
                            for _ in 0..4 {
                                if i < len {
                                    out.push(chars[i]);
                                    prev_char = Some(chars[i]);
                                    i += 1;
                                }
                            }
                        }
                    } else if next == 'x' {
                        for _ in 0..2 {
                            if i < len {
                                out.push(chars[i]);
                                prev_char = Some(chars[i]);
                                i += 1;
                            }
                        }
                    }
                } else if chars[i] == quote {
                    prev_char = Some(chars[i]);
                    i += 1;
                    state = State::Normal;
                } else {
                    prev_char = Some(chars[i]);
                    i += 1;
                }
            },
        }
    }

    out
}

/// Determine if a space is needed between two characters to avoid merging tokens.
fn needs_space_separator(prev: Option<char>, next: Option<char>) -> bool {
    match (prev, next) {
        (Some(p), Some(n)) => {
            (p.is_ascii_alphanumeric() || p == '_' || p == '$') && (n.is_ascii_alphanumeric() || n == '_' || n == '$')
        },
        _ => false,
    }
}

/// Scan JavaScript source for class/ID references used in DOM APIs.
///
/// Looks for patterns like:
/// - `document.getElementById("foo")`
/// - `document.querySelector(".bar")`
/// - `element.classList.add("baz")`
pub fn extract_js_references(js: &str, symbols: &mut SymbolMap, rename_classes: bool, rename_ids: bool) {
    if rename_ids {
        extract_function_string_args(js, "getElementById", |name| {
            symbols.register_id(name);
        });
    }

    if rename_classes {
        for func in &[
            "classList.add",
            "classList.remove",
            "classList.toggle",
            "classList.contains",
        ] {
            extract_function_string_args(js, func, |name| {
                symbols.register_class(name);
            });
        }
    }

    if rename_classes || rename_ids {
        for func in &["querySelector", "querySelectorAll"] {
            extract_function_string_args(js, func, |selector| {
                extract_selectors_from_query(selector, symbols, rename_classes, rename_ids);
            });
        }
    }
}

/// Extract string arguments from function calls like `funcName("value")`.
fn extract_function_string_args(js: &str, func_name: &str, mut callback: impl FnMut(&str)) {
    let mut search_from = 0;
    while let Some(pos) = js[search_from..].find(func_name) {
        let abs_pos = search_from + pos + func_name.len();
        let rest = &js[abs_pos..];

        let rest = rest.trim_start();
        if let Some(rest) = rest.strip_prefix('(') {
            let rest = rest.trim_start();
            if let Some(value) = extract_string_literal(rest) {
                callback(&value);
            }
        }
        search_from = abs_pos;
    }
}

/// Extract a string literal value from the start of a string slice.
fn extract_string_literal(s: &str) -> Option<String> {
    let s = s.trim_start();
    let quote = s.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }

    let mut value = String::new();
    let mut chars = s[1..].chars();
    loop {
        let ch = chars.next()?;
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                value.push(escaped);
            }
        } else if ch == quote {
            return Some(value);
        } else {
            value.push(ch);
        }
    }
}

/// Extract class/ID names from a CSS selector string (as used in querySelector).
fn extract_selectors_from_query(selector: &str, symbols: &mut SymbolMap, rename_classes: bool, rename_ids: bool) {
    let chars: Vec<char> = selector.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if chars[i] == '.' && rename_classes {
            i += 1;
            let start = i;
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '-' || chars[i] == '_') {
                i += 1;
            }
            if i > start {
                let name: String = chars[start..i].iter().collect();
                symbols.register_class(&name);
            }
        } else if chars[i] == '#' && rename_ids {
            i += 1;
            let start = i;
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '-' || chars[i] == '_') {
                i += 1;
            }
            if i > start {
                let name: String = chars[start..i].iter().collect();
                symbols.register_id(&name);
            }
        } else {
            i += 1;
        }
    }
}

/// Extract class name prefixes used in JS string concatenation.
///
/// Finds patterns like `'tier-' +` or `"tier-border-" +` and returns the
/// trailing CSS-name prefix (e.g., `tier-`, `tier-border-`).
pub fn extract_concatenation_prefixes(js: &str) -> Vec<String> {
    let mut prefixes = Vec::new();
    let chars: Vec<char> = js.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut state = State::Normal;

    while i < len {
        match state {
            State::Normal => {
                if i + 1 < len && chars[i] == '/' && chars[i + 1] == '/' {
                    state = State::SingleLineComment;
                    i += 2;
                } else if i + 1 < len && chars[i] == '/' && chars[i + 1] == '*' {
                    state = State::MultiLineComment;
                    i += 2;
                } else if chars[i] == '\'' || chars[i] == '"' {
                    let quote = chars[i];
                    i += 1;
                    let start = i;
                    while i < len {
                        if chars[i] == '\\' && i + 1 < len {
                            i += 2;
                        } else if chars[i] == quote {
                            break;
                        } else {
                            i += 1;
                        }
                    }
                    let content: String = chars[start..i].iter().collect();
                    if i < len {
                        i += 1; // skip closing quote
                    }
                    // Check if followed by +
                    let mut j = i;
                    while j < len && chars[j].is_ascii_whitespace() {
                        j += 1;
                    }
                    if j < len && chars[j] == '+' && content.ends_with('-') {
                        if let Some(prefix) = extract_trailing_prefix(&content) {
                            if !prefixes.contains(&prefix) {
                                prefixes.push(prefix);
                            }
                        }
                    }
                } else if chars[i] == '`' {
                    state = State::TemplateString;
                    i += 1;
                } else {
                    i += 1;
                }
            },
            State::SingleLineComment => {
                if chars[i] == '\n' {
                    state = State::Normal;
                }
                i += 1;
            },
            State::MultiLineComment => {
                if chars[i] == '*' && i + 1 < len && chars[i + 1] == '/' {
                    i += 2;
                    state = State::Normal;
                } else {
                    i += 1;
                }
            },
            State::TemplateString => {
                if chars[i] == '\\' && i + 1 < len {
                    i += 2;
                } else if chars[i] == '`' {
                    state = State::Normal;
                    i += 1;
                } else {
                    i += 1;
                }
            },
            _ => {
                i += 1;
            },
        }
    }

    prefixes
}

/// Extract the trailing CSS class name prefix from a string.
/// E.g., from `class="exec-banner tier-border-`, returns `tier-border-`.
fn extract_trailing_prefix(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let len = bytes.len();
    if len == 0 || bytes[len - 1] != b'-' {
        return None;
    }

    let mut start = len;
    while start > 0 {
        let ch = bytes[start - 1];
        if ch.is_ascii_alphanumeric() || ch == b'-' || ch == b'_' {
            start -= 1;
        } else {
            break;
        }
    }

    let prefix = &s[start..];
    if prefix.len() > 1 {
        Some(prefix.to_owned())
    } else {
        None
    }
}

/// Replace class/ID names in text using word-boundary matching.
/// Used for non-JS script content (JSON data) where class/ID names appear as values.
pub fn replace_symbols_word_boundary(
    text: &str,
    symbols: &crate::symbol_map::SymbolMap,
    rename_classes: bool,
    rename_ids: bool,
) -> String {
    let mut result = text.to_owned();
    if rename_classes {
        let mut class_pairs: Vec<_> = symbols.classes().iter().collect();
        class_pairs.sort_by_key(|b| std::cmp::Reverse(b.0.len()));
        for (original, obfuscated) in &class_pairs {
            result = replace_word(&result, original, obfuscated);
        }
    }
    if rename_ids {
        let mut id_pairs: Vec<_> = symbols.ids().iter().collect();
        id_pairs.sort_by_key(|b| std::cmp::Reverse(b.0.len()));
        for (original, obfuscated) in &id_pairs {
            result = replace_word(&result, original, obfuscated);
        }
    }
    result
}

/// Replace `word` with `replacement` only at class/ID name word boundaries.
///
/// A boundary exists where the adjacent character is NOT `[a-zA-Z0-9_-]`.
/// This prevents "critical" from matching inside "sev_critical".
fn replace_word(text: &str, word: &str, replacement: &str) -> String {
    if word.is_empty() {
        return text.to_owned();
    }
    let text_bytes = text.as_bytes();
    let word_bytes = word.as_bytes();
    let mut result = String::with_capacity(text.len());
    let mut search_from = 0;

    while let Some(pos) = text[search_from..].find(word) {
        let abs_pos = search_from + pos;
        let end_pos = abs_pos + word_bytes.len();

        let before_ok = abs_pos == 0 || !is_css_name_char(text_bytes[abs_pos - 1]);
        let after_ok = end_pos >= text_bytes.len() || !is_css_name_char(text_bytes[end_pos]);

        if before_ok && after_ok {
            result.push_str(&text[search_from..abs_pos]);
            result.push_str(replacement);
            search_from = end_pos;
        } else {
            // Not a word boundary match - advance past the first byte
            result.push_str(&text[search_from..abs_pos + 1]);
            search_from = abs_pos + 1;
        }
    }
    result.push_str(&text[search_from..]);
    result
}

fn is_css_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_string_literals() {
        use rand::SeedableRng;
        let mut rng = StdRng::seed_from_u64(42);
        let input = r#"var x = "hello";"#;
        let result = encode_js_strings(input, &mut rng);
        assert!(!result.contains("hello"));
        // At least one escape form must appear.
        assert!(result.contains("\\x") || result.contains("\\u"));
    }

    #[test]
    fn minify_removes_comments() {
        let input = "var x = 1; // comment\nvar y = 2;";
        let result = minify_js(input);
        assert!(!result.contains("comment"));
        assert!(result.contains("var x"));
    }

    #[test]
    fn minify_preserves_strings() {
        let input = r#"var x = "  spaces  ";"#;
        let result = minify_js(input);
        assert!(result.contains("  spaces  "));
    }

    #[test]
    fn extract_getelementbyid() {
        let js = r#"document.getElementById("myId");"#;
        let mut symbols = SymbolMap::new(Some(42));
        extract_js_references(js, &mut symbols, true, true);
        assert!(symbols.get_id("myId").is_some());
    }

    #[test]
    fn extract_classlist_add() {
        let js = r#"el.classList.add("active");"#;
        let mut symbols = SymbolMap::new(Some(42));
        extract_js_references(js, &mut symbols, true, true);
        assert!(symbols.get_class("active").is_some());
    }

    #[test]
    fn extract_queryselector() {
        let js = r#"document.querySelector(".foo #bar");"#;
        let mut symbols = SymbolMap::new(Some(42));
        extract_js_references(js, &mut symbols, true, true);
        assert!(symbols.get_class("foo").is_some());
        assert!(symbols.get_id("bar").is_some());
    }

    #[test]
    fn replace_references_in_strings() {
        let js = r#"var cls = "myClass";"#;
        let mut symbols = SymbolMap::new(Some(42));
        symbols.register_class("myClass");
        let obf = symbols.get_class("myClass").unwrap().to_owned();
        let result = replace_symbol_references(js, &symbols, true, false);
        assert!(result.contains(&obf));
        assert!(!result.contains("myClass"));
    }
}
