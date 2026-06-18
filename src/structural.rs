//! Structural obfuscation with client-side restoration (WebCloak-style):
//! visible text in safe flow elements is removed from the static markup and
//! stashed base64-encoded in a `data-ssk` attribute, then restored at runtime
//! by an injected script. Static scrapers see only opaque base64; a browser
//! renders identically. Strictly opt-in: it breaks no-JS, SEO, and (until
//! restoration runs) accessibility.

/// Tags whose *direct* text children may be wrapped in a `<span>` without
/// producing invalid markup. Deliberately conservative: metadata containers
/// (`title`, `option`, `textarea`), raw-text elements, and head content are
/// excluded, as are whitespace-preserving `pre`/`code`.
const SAFE_TAGS: &[&str] = &[
    "p",
    "div",
    "li",
    "td",
    "th",
    "dd",
    "dt",
    "caption",
    "figcaption",
    "blockquote",
    "section",
    "article",
    "aside",
    "main",
    "header",
    "footer",
    "nav",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "span",
    "a",
    "b",
    "i",
    "em",
    "strong",
    "small",
    "mark",
    "label",
    "button",
    "summary",
    "cite",
    "q",
    "abbr",
    "time",
    "address",
    "dfn",
];

/// Whether text directly inside `tag` can be safely relocated into a span.
pub fn is_safe_tag(tag: &str) -> bool {
    SAFE_TAGS.contains(&tag)
}

/// Encode a text node's content into a hidden `<span data-ssk="...">`.
///
/// The visible text is replaced by base64(UTF-8) held in the attribute; the
/// element renders empty until the restore script runs.
pub fn encode_text_node(text: &str) -> String {
    format!("<span data-ssk=\"{}\"></span>", base64_encode(text.as_bytes()))
}

/// The runtime restoration script (minified). Decodes every `data-ssk`
/// attribute back into `textContent` using a UTF-8-correct base64 decode.
pub fn restore_script() -> &'static str {
    "<script>(function(){function d(b){var s=atob(b),a=new Uint8Array(s.length);\
for(var i=0;i<s.length;i++)a[i]=s.charCodeAt(i);return new TextDecoder().decode(a);}\
document.querySelectorAll('[data-ssk]').forEach(function(e){\
try{e.textContent=d(e.getAttribute('data-ssk'));e.removeAttribute('data-ssk');}catch(_){}});})();</script>"
}

/// Standard base64 (RFC 4648) encoder - std-only, no extra dependency.
fn base64_encode(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    }

    #[test]
    fn base64_handles_utf8() {
        // Multibyte UTF-8 input must base64 to pure ASCII (browser decodes via TextDecoder).
        let enc = base64_encode("안녕".as_bytes());
        assert!(!enc.is_empty());
        assert!(enc.is_ascii());
    }

    #[test]
    fn encoded_node_hides_plaintext() {
        let span = encode_text_node("secret message");
        assert!(!span.contains("secret message"));
        assert!(span.contains("data-ssk="));
    }

    #[test]
    fn safe_tag_set() {
        assert!(is_safe_tag("p"));
        assert!(is_safe_tag("span"));
        assert!(!is_safe_tag("title"));
        assert!(!is_safe_tag("textarea"));
        assert!(!is_safe_tag("script"));
    }
}
