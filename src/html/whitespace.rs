/// Tags whose content whitespace must be preserved.
const PRESERVED_TAGS: &[&str] = &["pre", "code", "textarea", "script", "style"];

/// Check if a tag name requires whitespace preservation.
pub fn is_preserved_tag(tag: &str) -> bool {
    PRESERVED_TAGS.contains(&tag.to_ascii_lowercase().as_str())
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
}
