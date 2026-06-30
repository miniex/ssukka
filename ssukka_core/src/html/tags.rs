use rand::rngs::StdRng;
use rand::RngExt;

/// Randomize the case of each character in a tag name.
///
/// `div` might become `DiV`, `dIv`, etc.
pub fn randomize_tag_case(tag: &str, rng: &mut StdRng) -> String {
    tag.chars()
        .map(|ch| {
            if rng.random_bool(0.5) {
                ch.to_ascii_uppercase()
            } else {
                ch.to_ascii_lowercase()
            }
        })
        .collect()
}

/// Reorder attributes by a per-document salted key.
///
/// Sorting by `key(salt, name)` differs from source order yet is identical for
/// any two elements sharing an attribute set, so repeated tag shapes still
/// compress (a per-element random shuffle would defeat gzip).
pub fn shuffle_attributes(attrs: &mut [(String, String)], salt: u64) {
    attrs.sort_by_key(|(name, _)| attr_order_key(name, salt));
}

/// Stable per-name ordering key: FNV-1a hash of `name` salted with `salt`.
fn attr_order_key(name: &str, salt: u64) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64 ^ salt;
    for b in name.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn tag_case_randomization() {
        let mut rng = StdRng::seed_from_u64(42);
        let result = randomize_tag_case("div", &mut rng);
        assert_eq!(result.to_ascii_lowercase(), "div");
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn attribute_shuffle_preserves_set() {
        let mut attrs = vec![
            ("a".into(), "1".into()),
            ("b".into(), "2".into()),
            ("c".into(), "3".into()),
            ("d".into(), "4".into()),
        ];
        let original: Vec<_> = attrs.clone();
        shuffle_attributes(&mut attrs, 0xdead_beef);
        assert_eq!(attrs.len(), original.len());
        for item in &original {
            assert!(attrs.contains(item));
        }
    }

    #[test]
    fn attribute_order_is_document_stable() {
        // Two elements sharing attribute names must reorder identically under
        // the same salt, so repeated tag shapes still compress.
        let salt = 0x1234_5678;
        let mut a = vec![("class".into(), "x".into()), ("id".into(), "y".into())];
        let mut b = vec![("class".into(), "p".into()), ("id".into(), "q".into())];
        shuffle_attributes(&mut a, salt);
        shuffle_attributes(&mut b, salt);
        let names_a: Vec<&String> = a.iter().map(|(n, _)| n).collect();
        let names_b: Vec<&String> = b.iter().map(|(n, _)| n).collect();
        assert_eq!(names_a, names_b);
    }

    #[test]
    fn deterministic_tag_case() {
        let mut rng1 = StdRng::seed_from_u64(42);
        let mut rng2 = StdRng::seed_from_u64(42);
        assert_eq!(
            randomize_tag_case("section", &mut rng1),
            randomize_tag_case("section", &mut rng2),
        );
    }
}
