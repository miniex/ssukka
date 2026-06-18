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

/// Shuffle the order of attributes in an element.
///
/// Takes a list of (name, value) pairs and returns them in random order.
pub fn shuffle_attributes(attrs: &mut [(String, String)], rng: &mut StdRng) {
    // Fisher-Yates shuffle
    let len = attrs.len();
    for i in (1..len).rev() {
        let j = rng.random_range(0..=i);
        attrs.swap(i, j);
    }
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
    fn attribute_shuffle() {
        let mut rng = StdRng::seed_from_u64(42);
        let mut attrs = vec![
            ("a".into(), "1".into()),
            ("b".into(), "2".into()),
            ("c".into(), "3".into()),
            ("d".into(), "4".into()),
        ];
        let original: Vec<_> = attrs.clone();
        shuffle_attributes(&mut attrs, &mut rng);
        assert_eq!(attrs.len(), original.len());
        for item in &original {
            assert!(attrs.contains(item));
        }
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
