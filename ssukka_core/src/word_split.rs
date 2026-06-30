//! Word splitting: insert an empty HTML comment inside long words so a naive
//! regex/substring scraper sees fragmented text, while browsers, readers, and
//! content extractors merge across the comment and read the word intact. Flow
//! content only (in `<title>` and other RCDATA a comment is literal text).

use crate::html::entities;
use rand::rngs::StdRng;
use rand::RngExt;

/// Inserted between word fragments. A comment renders nothing and adds no
/// whitespace, so the word looks unbroken; raw-HTML scrapers see the split.
const MARKER: &str = "<!-- -->";

/// Split `text` by inserting [`MARKER`] inside each word of >= 4 chars, entity-
/// encoding each resulting segment when `encode` is set (so the marker lands
/// between segments, never inside an entity).
pub fn split(text: &str, encode: bool, rng: &mut StdRng) -> String {
    let positions = split_positions(text, rng);
    if positions.is_empty() {
        return maybe_encode(text, encode, rng);
    }
    let mut out = String::with_capacity(text.len() + positions.len() * MARKER.len());
    let mut last = 0;
    for p in positions {
        out.push_str(&maybe_encode(&text[last..p], encode, rng));
        out.push_str(MARKER);
        last = p;
    }
    out.push_str(&maybe_encode(&text[last..], encode, rng));
    out
}

fn maybe_encode(seg: &str, encode: bool, rng: &mut StdRng) -> String {
    if encode {
        entities::encode_entities(seg, rng)
    } else {
        seg.to_owned()
    }
}

/// One interior byte offset per whitespace-delimited word of >= 4 chars.
fn split_positions(text: &str, rng: &mut StdRng) -> Vec<usize> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let n = chars.len();
    let mut positions = Vec::new();
    let mut i = 0;
    while i < n {
        if chars[i].1.is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && !chars[i].1.is_whitespace() {
            i += 1;
        }
        let len = i - start;
        if len >= 4 {
            // An interior boundary: between char start+1 and end-1.
            let pick = start + 1 + rng.random_range(0..(len - 2));
            positions.push(chars[pick].0);
        }
    }
    positions
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn splits_long_words_only() {
        let mut rng = StdRng::seed_from_u64(1);
        let out = split("hi the quick", false, &mut rng);
        // "hi" (2) and "the" (3) are too short; "quick" (5) gets one marker.
        assert_eq!(out.matches(MARKER).count(), 1);
        assert!(out.contains("hi the"), "short words untouched: {out}");
    }

    #[test]
    fn removing_markers_restores_text() {
        let mut rng = StdRng::seed_from_u64(7);
        let text = "alpha bravo charlie delta";
        let out = split(text, false, &mut rng);
        assert_eq!(out.replace(MARKER, ""), text);
    }

    #[test]
    fn markers_land_between_entities_when_encoded() {
        let mut rng = StdRng::seed_from_u64(3);
        let out = split("password", true, &mut rng);
        // No marker may split an `&#...;` entity (would corrupt it).
        for frag in out.split(MARKER) {
            let amps = frag.matches('&').count();
            let semis = frag.matches(';').count();
            assert_eq!(amps, semis, "entity split across marker: {out}");
        }
    }
}
