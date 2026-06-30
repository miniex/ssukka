//! Word splitting: insert an empty HTML comment inside long words so a naive
//! regex/substring scraper sees fragmented text, while browsers, readers, and
//! content extractors merge across the comment and read the word intact. Flow
//! content only (in `<title>` and other RCDATA a comment is literal text).

use crate::html::entities;
use rand::rngs::StdRng;

/// Inserted between word fragments. A comment renders nothing and adds no
/// whitespace, so the word looks unbroken; raw-HTML scrapers see the split.
const MARKER: &str = "<!-- -->";

/// Roughly one marker per this many chars inside a run.
const STEP: usize = 6;

/// Split `text` by inserting [`MARKER`] inside each run of >= 4 non-whitespace
/// chars (about one per [`STEP`] chars, so long words and space-free scripts like
/// CJK fragment throughout). Each segment is entity-encoded when `encode` is set,
/// so a marker never lands inside an entity.
pub fn split(text: &str, encode: bool, rng: &mut StdRng) -> String {
    let positions = split_positions(text);
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

/// Interior byte offsets for each whitespace-delimited run of >= 4 chars: about
/// `len / STEP` markers (at least one), spread evenly between the run's ends.
fn split_positions(text: &str) -> Vec<usize> {
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
            let count = (len / STEP).max(1);
            for k in 1..=count {
                // Evenly spaced interior boundaries (between char start+1 and end-1).
                let off = (start + len * k / (count + 1)).clamp(start + 1, i - 1);
                positions.push(chars[off].0);
            }
        }
    }
    positions.dedup();
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
    fn long_runs_get_multiple_markers() {
        let mut rng = StdRng::seed_from_u64(2);
        // 18 chars -> len/STEP = 3 markers.
        let out = split("abcdefghijklmnopqr", false, &mut rng);
        assert_eq!(out.matches(MARKER).count(), 3, "{out}");
        assert_eq!(out.replace(MARKER, ""), "abcdefghijklmnopqr");
        // Space-free CJK run is fragmented throughout, not just once.
        let cjk = "동해물과백두산이마르고닳도록"; // 14 chars, no whitespace
        let out2 = split(cjk, false, &mut rng);
        assert!(out2.matches(MARKER).count() >= 2, "CJK run fragmented: {out2}");
        assert_eq!(out2.replace(MARKER, ""), cjk);
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
