//! Structural obfuscation with client-side restoration (WebCloak-style):
//! visible text in safe flow elements is moved out of the static markup into an
//! encoded `data-*` attribute, then restored at runtime by an injected script.
//! Static scrapers see only opaque text; a browser renders identically.
//! Opt-in: breaks no-JS, SEO, and (until restore runs) accessibility.
//!
//! The encoding is polymorphic per build (random attribute name + XOR key +
//! byte order via a [`Scheme`]), so no fixed decoder recipe works.

use rand::rngs::StdRng;
use rand::RngExt;

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

/// Per-document encoding parameters shared by [`Scheme::encode_text_node`] and
/// the restore script. Drawn once so all nodes in a document use one scheme.
pub struct Scheme {
    /// Payload attribute (`data-` + random suffix): random so a fixed selector
    /// can't find it, distinctive enough not to collide with a real attribute.
    attr: String,
    /// Cyclic XOR key applied before base64, so the payload is not plain text.
    key: Vec<u8>,
    /// Whether bytes are reversed before encoding.
    reverse: bool,
}

impl Scheme {
    /// Draw a fresh scheme from `rng` (deterministic under a seed).
    pub fn new(rng: &mut StdRng) -> Self {
        let mut attr = String::from("data-");
        for _ in 0..6 {
            attr.push((b'a' + rng.random_range(0..26)) as char);
        }
        let key_len = rng.random_range(1..=3);
        let key = (0..key_len).map(|_| rng.random_range(1..=255)).collect();
        let reverse = rng.random_bool(0.5);
        Self { attr, key, reverse }
    }

    /// Encode `text` into a hidden `<span data-...="...">`. The element renders
    /// empty until the restore script decodes the attribute back into its text.
    pub fn encode_text_node(&self, text: &str) -> String {
        let mut bytes = text.as_bytes().to_vec();
        if self.reverse {
            bytes.reverse();
        }
        for (i, b) in bytes.iter_mut().enumerate() {
            *b ^= self.key[i % self.key.len()];
        }
        format!("<span {}=\"{}\"></span>", self.attr, base64_encode(&bytes))
    }

    /// The runtime restoration script with this scheme's parameters baked in:
    /// base64-decode, un-XOR, un-reverse, then UTF-8 decode into `textContent`.
    pub fn restore_script(&self) -> String {
        let attr = &self.attr;
        let key = self.key.iter().map(u8::to_string).collect::<Vec<_>>().join(",");
        // Destination index inverts the optional encode-time reverse.
        let idx = if self.reverse { "n-1-i" } else { "i" };
        format!(
            "<script>(function(){{var K=[{key}];\
function d(b){{var s=atob(b),n=s.length,a=new Uint8Array(n);\
for(var i=0;i<n;i++)a[{idx}]=s.charCodeAt(i)^K[i%K.length];\
return new TextDecoder().decode(a);}}\
document.querySelectorAll('[{attr}]').forEach(function(e){{\
try{{e.textContent=d(e.getAttribute('{attr}'));e.removeAttribute('{attr}');}}catch(_){{}}}});}})();</script>"
        )
    }
}

/// Standard base64 (RFC 4648) encoder, std-only with no extra dependency.
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
    use rand::SeedableRng;

    /// Base64 decoder mirroring the restore script's `atob`, for roundtrip tests.
    fn base64_decode(s: &str) -> Vec<u8> {
        fn val(c: u8) -> u8 {
            match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                b'/' => 63,
                _ => 0,
            }
        }
        let chars: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
        let mut out = Vec::with_capacity(chars.len() * 3 / 4);
        for chunk in chars.chunks(4) {
            let mut buf = [0u32; 4];
            for (i, &c) in chunk.iter().enumerate() {
                buf[i] = val(c) as u32;
            }
            let b = (buf[0] << 18) | (buf[1] << 12) | (buf[2] << 6) | buf[3];
            if chunk.len() >= 2 {
                out.push((b >> 16) as u8);
            }
            if chunk.len() >= 3 {
                out.push((b >> 8) as u8);
            }
            if chunk.len() >= 4 {
                out.push(b as u8);
            }
        }
        out
    }

    /// Inverse of [`Scheme::encode_text_node`], mirroring the restore JS.
    fn decode(scheme: &Scheme, b64: &str) -> String {
        let b2 = base64_decode(b64);
        let n = b2.len();
        let mut b0 = vec![0u8; n];
        for (i, &byte) in b2.iter().enumerate() {
            let v = byte ^ scheme.key[i % scheme.key.len()];
            b0[if scheme.reverse { n - 1 - i } else { i }] = v;
        }
        String::from_utf8(b0).unwrap()
    }

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
    fn encode_decode_roundtrip_all_schemes() {
        let texts = ["hello", "x", "Multi-byte: cafe \u{2615} 안녕", "a b  c"];
        for seed in [1u64, 2, 7, 42, 100] {
            let mut rng = StdRng::seed_from_u64(seed);
            let scheme = Scheme::new(&mut rng);
            for t in texts {
                let node = scheme.encode_text_node(t);
                let b64 = node.split('"').nth(1).unwrap();
                assert_eq!(decode(&scheme, b64), t, "roundtrip failed (seed {seed}) for {t:?}");
            }
        }
    }

    #[test]
    fn encoded_node_hides_plaintext_with_random_attr() {
        let mut rng = StdRng::seed_from_u64(1);
        let scheme = Scheme::new(&mut rng);
        let node = scheme.encode_text_node("secret message");
        assert!(!node.contains("secret message"));
        assert!(node.contains("<span data-"));
        assert!(!node.contains("data-ssk"), "fixed attr name must not be used");
    }

    #[test]
    fn restore_script_carries_scheme_params() {
        let mut rng = StdRng::seed_from_u64(3);
        let scheme = Scheme::new(&mut rng);
        let js = scheme.restore_script();
        assert!(js.contains("TextDecoder"));
        assert!(
            js.contains(&format!("[{}]", scheme.attr)),
            "selector must use the random attr"
        );
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
