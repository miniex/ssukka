//! Invisible per-build watermark: a 64-bit id as zero-width characters, scattered
//! as redundant copies through the body text so a scraped or partially-copied
//! page can be traced. Renders invisibly; recovered by majority vote.
//!
//! Opt-in: zero-width chars can affect screen readers and programmatic matching,
//! so copies go only in normal flow text (never attributes/script/style/pre).

/// Zero-width space: bit 0.
const ZERO: char = '\u{200b}';
/// Zero-width non-joiner: bit 1.
const ONE: char = '\u{200c}';
/// Word joiner: frames the bit sequence so the decoder can locate it.
const MARK: char = '\u{2060}';

/// Redundant copies to scatter per document, so partial deletion still leaves
/// enough complete frames to recover the id by majority vote.
pub const COPIES: usize = 8;

/// One word-joiner-framed 64-bit copy of `id` (zero-width). Scatter several
/// across the document for redundancy; [`decode`] majority-votes them.
pub fn embed(id: u64) -> String {
    let mut s = String::with_capacity(66);
    s.push(MARK);
    for i in (0..64).rev() {
        s.push(if (id >> i) & 1 == 1 { ONE } else { ZERO });
    }
    s.push(MARK);
    s
}

/// Recover the id by majority-voting every complete 64-bit frame in `text`, so it
/// tolerates missing copies (deletion) and flipped bits (corruption). `None` if
/// no complete frame is present.
pub fn decode(text: &str) -> Option<u64> {
    let frames = collect_frames(text);
    if frames.is_empty() {
        return None;
    }
    let mut id = 0u64;
    for bit in 0..64 {
        let ones = frames.iter().filter(|&&f| (f >> bit) & 1 == 1).count();
        if ones * 2 > frames.len() {
            id |= 1 << bit;
        }
    }
    Some(id)
}

/// Every complete word-joiner-framed 64-bit run in `text`, in document order.
fn collect_frames(text: &str) -> Vec<u64> {
    let mut frames = Vec::new();
    let mut in_frame = false;
    let mut bits: u64 = 0;
    let mut count = 0u32;
    for ch in text.chars() {
        match ch {
            MARK => {
                if in_frame && count == 64 {
                    frames.push(bits);
                }
                in_frame = true;
                bits = 0;
                count = 0;
            },
            ZERO if in_frame && count < 64 => {
                bits <<= 1;
                count += 1;
            },
            ONE if in_frame && count < 64 => {
                bits = (bits << 1) | 1;
                count += 1;
            },
            _ if in_frame => in_frame = false,
            _ => {},
        }
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_through_text() {
        let id = 0xDEAD_BEEF_1234_5678;
        let marked = format!("Hello {}world", embed(id));
        assert_eq!(decode(&marked), Some(id));
    }

    #[test]
    fn marker_is_invisible_and_zero_width() {
        let s = embed(1);
        assert_eq!(s.chars().count(), 66); // 2 markers + 64 bits
        assert!(s.chars().all(|c| matches!(c, ZERO | ONE | MARK)));
    }

    #[test]
    fn absent_watermark_decodes_to_none() {
        assert_eq!(decode("plain text, no watermark"), None);
    }

    #[test]
    fn roundtrips_zero_and_max() {
        assert_eq!(decode(&embed(0)), Some(0));
        assert_eq!(decode(&embed(u64::MAX)), Some(u64::MAX));
    }

    #[test]
    fn recovers_id_after_partial_removal() {
        let id = 0xDEAD_BEEF_1234_5678;
        // Several copies scattered through the text, each in its own segment.
        let copies: Vec<String> = (0..6).map(|i| format!("para {i} {}", embed(id))).collect();
        // A scraper keeps only the tail half: the early copies are gone.
        let survived = copies[3..].join(" ");
        assert_eq!(decode(&survived), Some(id), "surviving copies must still decode");
    }

    #[test]
    fn majority_vote_survives_a_corrupted_copy() {
        // 3 good copies vs 1 with its low bits flipped -> majority recovers the id.
        let id = 0x0123_4567_89AB_CDEF;
        let good = embed(id);
        let bad = embed(id ^ 0xFFFF);
        let text = format!("{good}x{good}y{good}z{bad}");
        assert_eq!(decode(&text), Some(id));
    }

    #[test]
    fn multiple_identical_copies_decode_to_id() {
        let id = 42;
        let text = format!("a{w}b{w}c{w}", w = embed(id));
        assert_eq!(decode(&text), Some(id));
    }
}
