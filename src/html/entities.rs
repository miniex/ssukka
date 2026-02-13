use rand::rngs::StdRng;
use rand::RngExt;

/// Encode a string into a mix of HTML entities (decimal, hex, named).
///
/// Each character is randomly encoded using one of three forms,
/// making pattern recognition harder while remaining valid HTML.
pub fn encode_entities(text: &str, rng: &mut StdRng) -> String {
    let mut out = String::with_capacity(text.len() * 6);
    for ch in text.chars() {
        if let Some(named) = named_entity(ch) {
            match rng.random_range(0u8..3) {
                0 => out.push_str(named),
                1 => encode_decimal(ch, &mut out),
                _ => encode_hex(ch, &mut out),
            }
        } else if ch.is_ascii_alphanumeric() || ch == ' ' {
            // Randomly encode or pass through for common chars
            match rng.random_range(0u8..3) {
                0 => out.push(ch),
                1 => encode_decimal(ch, &mut out),
                _ => encode_hex(ch, &mut out),
            }
        } else {
            // Non-ASCII or special chars — always encode
            match rng.random_range(0u8..2) {
                0 => encode_decimal(ch, &mut out),
                _ => encode_hex(ch, &mut out),
            }
        }
    }
    out
}

/// Encode a single attribute value into entities.
/// Preserves structure needed for attribute parsing.
pub fn encode_attr_value(value: &str, rng: &mut StdRng) -> String {
    let mut out = String::with_capacity(value.len() * 6);
    for ch in value.chars() {
        match rng.random_range(0u8..3) {
            0 => out.push(ch),
            1 => encode_decimal(ch, &mut out),
            _ => encode_hex(ch, &mut out),
        }
    }
    out
}

fn encode_decimal(ch: char, out: &mut String) {
    out.push_str(&format!("&#{};", ch as u32));
}

fn encode_hex(ch: char, out: &mut String) {
    out.push_str(&format!("&#x{:x};", ch as u32));
}

fn named_entity(ch: char) -> Option<&'static str> {
    match ch {
        '&' => Some("&amp;"),
        '<' => Some("&lt;"),
        '>' => Some("&gt;"),
        '"' => Some("&quot;"),
        '\'' => Some("&#39;"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn encode_entities_roundtrip_safe() {
        let mut rng = StdRng::seed_from_u64(42);
        let input = "Hello <World> & 'friends'";
        let encoded = encode_entities(input, &mut rng);
        // Should not contain raw < or > (they get entity-encoded)
        // The encoded form should be valid HTML entities
        assert!(!encoded.contains('<'));
        assert!(!encoded.contains('>'));
    }

    #[test]
    fn deterministic_with_same_seed() {
        let mut rng1 = StdRng::seed_from_u64(99);
        let mut rng2 = StdRng::seed_from_u64(99);
        let input = "test string";
        assert_eq!(
            encode_entities(input, &mut rng1),
            encode_entities(input, &mut rng2),
        );
    }
}
