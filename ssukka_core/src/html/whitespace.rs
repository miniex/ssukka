//! Per-element whitespace handling. Three classes:
//! - **Preserve** ([`is_preserved_tag`]): whitespace is significant, leave it.
//! - **Whitespace-container** ([`is_whitespace_container`]): direct
//!   whitespace-only text never renders (table/select internals), drop it.
//! - everything else: collapse runs to a single space ([`collapse_whitespace`]).
//!
//! minify-html's aggressive rules (trim block edges, destroy in any layout
//! element) need sibling lookahead to stay render-correct, which a streaming
//! pass lacks - so collapse-to-one-space is the safe default for that class.

/// Tags whose content whitespace must be preserved.
const PRESERVED_TAGS: &[&str] = &["pre", "code", "textarea", "script", "style"];

/// Container elements whose direct whitespace-only text children never render -
/// the HTML parser foster-parents or ignores them regardless of CSS - so such
/// nodes can be dropped outright. Excludes list containers (`ul`/`ol`/`dl`),
/// where an inline-block child could turn inter-item whitespace into a gap.
const WHITESPACE_CONTAINERS: &[&str] = &[
    "table", "thead", "tbody", "tfoot", "tr", "colgroup", "select", "optgroup", "datalist",
];

/// Check if a tag name requires whitespace preservation.
pub fn is_preserved_tag(tag: &str) -> bool {
    PRESERVED_TAGS.contains(&tag.to_ascii_lowercase().as_str())
}

/// Whether a (lowercased) tag drops its direct whitespace-only text children.
pub fn is_whitespace_container(tag: &str) -> bool {
    WHITESPACE_CONTAINERS.contains(&tag)
}

/// Collapse runs of whitespace in text content to a single space.
///
/// Leading/trailing whitespace is collapsed, not stripped: one space is kept
/// so adjacent inline elements don't run together.
pub fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_ws = false;

    for ch in text.chars() {
        if ch.is_ascii_whitespace() {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_multiple_spaces() {
        assert_eq!(collapse_whitespace("hello   world"), "hello world");
    }

    #[test]
    fn collapse_tabs_and_newlines() {
        assert_eq!(collapse_whitespace("hello\n\t  world"), "hello world");
    }

    #[test]
    fn preserves_single_spaces() {
        assert_eq!(collapse_whitespace("a b c"), "a b c");
    }

    #[test]
    fn preserved_tag_check() {
        assert!(is_preserved_tag("pre"));
        assert!(is_preserved_tag("PRE"));
        assert!(is_preserved_tag("code"));
        assert!(!is_preserved_tag("div"));
        assert!(!is_preserved_tag("span"));
    }

    #[test]
    fn whitespace_container_check() {
        assert!(is_whitespace_container("table"));
        assert!(is_whitespace_container("tr"));
        assert!(is_whitespace_container("select"));
        // List containers excluded: inline-block children could show a gap.
        assert!(!is_whitespace_container("ul"));
        assert!(!is_whitespace_container("ol"));
        assert!(!is_whitespace_container("div"));
        assert!(!is_whitespace_container("p"));
    }
}
