//! Invisible per-build watermark: a 64-bit id encoded as zero-width characters,
//! embedded once in the document text so a scraped/leaked copy can be traced.
//! Renders invisibly and survives copy-paste.
//!
//! Opt-in: zero-width characters can affect screen readers and programmatic
//! matching, so the id is embedded once (never in attributes/script/style/pre).

/// Zero-width space: bit 0.
const ZERO: char = '\u{200b}';
/// Zero-width non-joiner: bit 1.
const ONE: char = '\u{200c}';
/// Word joiner: frames the bit sequence so the decoder can locate it.
const MARK: char = '\u{2060}';

/// The zero-width sequence encoding `id` (a word-joiner-framed 64-bit run).
pub fn embed(id: u64) -> String {
    let mut s = String::with_capacity(66);
    s.push(MARK);
    for i in (0..64).rev() {
        s.push(if (id >> i) & 1 == 1 { ONE } else { ZERO });
    }
    s.push(MARK);
    s
}

/// Recover the first embedded id from `text`, or `None` if none is present.
pub fn decode(text: &str) -> Option<u64> {
    let mut in_frame = false;
    let mut bits: u64 = 0;
    let mut count = 0u32;
    for ch in text.chars() {
        match ch {
            MARK => {
                if in_frame && count == 64 {
                    return Some(bits);
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
    None
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
}
