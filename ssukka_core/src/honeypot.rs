//! Honeypot / decoy injection: invisible trap links, fake form fields, and
//! decoy article-like prose blocks that waste scraper effort and poison harvested
//! data. Decoys are hidden from layout and assistive tech, so real users, keyboard
//! nav, and screen readers are unaffected.
//!
//! Each decoy carries a random marker; an injected script removes them on load,
//! so JS clients (real users, JS-rendering scrapers) get a clean DOM with no
//! signature, while no-JS bulk crawlers still ingest the decoys and take the bait.
//!
//! Vocabulary lives in `assets/honeypot/*.txt`, embedded via [`crate::wordlist`].

use crate::wordlist;
use rand::rngs::StdRng;
use rand::RngExt;
use std::sync::LazyLock;

/// Name fragments for decoy classes / fields.
static WORDS: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| wordlist::parse(include_str!("../assets/honeypot/classnames.txt")));

/// Trap path segments for decoy links.
static PATHS: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| wordlist::parse(include_str!("../assets/honeypot/paths.txt")));

/// Container class names that rule-based extractors (Readability/trafilatura)
/// score as main content, so the decoy prose block looks like the article.
static CONTENT_CLASSES: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| wordlist::parse(include_str!("../assets/honeypot/content-classes.txt")));

/// Vocabulary for synthesizing grammatical-but-meaningless decoy prose.
static PROSE_NOUNS: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| wordlist::parse(include_str!("../assets/honeypot/prose-nouns.txt")));
static PROSE_VERBS: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| wordlist::parse(include_str!("../assets/honeypot/prose-verbs.txt")));
static PROSE_ADJS: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| wordlist::parse(include_str!("../assets/honeypot/prose-adjectives.txt")));
static PROSE_CONN: LazyLock<Vec<&'static str>> =
    LazyLock::new(|| wordlist::parse(include_str!("../assets/honeypot/prose-connectors.txt")));

/// Per-document honeypot config: a random marker attribute tying the decoys to
/// their client-side removal script.
pub struct Honeypots {
    marker: String,
}

impl Honeypots {
    /// Draw a fresh marker from `rng` (deterministic under a seed).
    pub fn new(rng: &mut StdRng) -> Self {
        let mut marker = String::from("data-");
        for _ in 0..6 {
            marker.push((b'a' + rng.random_range(0..26)) as char);
        }
        Self { marker }
    }

    /// Build `count` invisible decoy nodes, each tagged with the marker.
    /// Returned verbatim (not re-processed by the rewriter), so names stay literal.
    pub fn generate(&self, count: usize, rng: &mut StdRng) -> String {
        let mut out = String::with_capacity(count * 192);
        for _ in 0..count {
            match rng.random_range(0u8..3) {
                0 => self.decoy_link(&mut out, rng),
                1 => self.decoy_field(&mut out, rng),
                _ => self.decoy_block(&mut out, rng),
            }
        }
        out
    }

    /// Script that deletes the marked decoys on load, leaving JS clients a clean
    /// DOM with no honeypot signature to fingerprint.
    pub fn removal_script(&self) -> String {
        format!(
            "<script>document.querySelectorAll('[{m}]').forEach(function(e){{e.remove();}});</script>",
            m = self.marker
        )
    }

    /// Inert styling (`display:none` keeps it out of layout and the a11y tree)
    /// plus the marker the removal script deletes for JS clients.
    fn hidden(&self) -> String {
        format!(
            r#"style="display:none!important" aria-hidden="true" tabindex="-1" {}"#,
            self.marker
        )
    }

    /// A trap link: crawlers that follow every href hit a bogus internal path.
    fn decoy_link(&self, out: &mut String, rng: &mut StdRng) {
        let path = pick(&PATHS, rng);
        let token = rand_token(rng, 8);
        let text = format!("{} {}", word(rng), word(rng));
        out.push_str(&format!(
            r#"<a href="/{path}/{token}" {h} rel="nofollow">{text}</a>"#,
            h = self.hidden(),
        ));
    }

    /// A honeypot form field: spam bots that auto-fill every input reveal themselves.
    fn decoy_field(&self, out: &mut String, rng: &mut StdRng) {
        let name = format!("{}_{}", word(rng), word(rng));
        out.push_str(&format!(
            r#"<input type="text" name="{name}" {h} autocomplete="off" value="">"#,
            h = self.hidden(),
        ));
    }

    /// A decoy "main content" block: dense, link-free prose in a content-like
    /// class, so an extractor starved of the real body (e.g. by structural
    /// obfuscation) scores it as the article and harvests the filler. Hidden + marked.
    fn decoy_block(&self, out: &mut String, rng: &mut StdRng) {
        let cls = pick(&CONTENT_CLASSES, rng);
        out.push_str(&format!(r#"<div class="{cls}" {h}>"#, h = self.hidden()));
        for _ in 0..rng.random_range(1..=3) {
            out.push_str("<p>");
            for i in 0..rng.random_range(2..=4) {
                if i > 0 {
                    out.push(' ');
                }
                self.decoy_sentence(out, rng);
            }
            out.push_str("</p>");
        }
        out.push_str("</div>");
    }

    /// One grammatical-but-meaningless sentence: `The <adj> <noun> <verb> <adj>
    /// <noun>[, <conn> the <adj> <noun> <verb>].`
    fn decoy_sentence(&self, out: &mut String, rng: &mut StdRng) {
        out.push_str("The ");
        out.push_str(pick(&PROSE_ADJS, rng));
        out.push(' ');
        out.push_str(pick(&PROSE_NOUNS, rng));
        out.push(' ');
        out.push_str(pick(&PROSE_VERBS, rng));
        out.push(' ');
        out.push_str(pick(&PROSE_ADJS, rng));
        out.push(' ');
        out.push_str(pick(&PROSE_NOUNS, rng));
        if rng.random_bool(0.5) {
            out.push_str(", ");
            out.push_str(pick(&PROSE_CONN, rng));
            out.push_str(" the ");
            out.push_str(pick(&PROSE_ADJS, rng));
            out.push(' ');
            out.push_str(pick(&PROSE_NOUNS, rng));
            out.push(' ');
            out.push_str(pick(&PROSE_VERBS, rng));
        }
        out.push('.');
    }
}

fn word(rng: &mut StdRng) -> &'static str {
    pick(&WORDS, rng)
}

fn pick(arr: &[&'static str], rng: &mut StdRng) -> &'static str {
    arr[rng.random_range(0..arr.len())]
}

/// Random lowercase-alphanumeric token.
fn rand_token(rng: &mut StdRng, len: usize) -> String {
    const CH: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    (0..len).map(|_| CH[rng.random_range(0..CH.len())] as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn generates_requested_count_of_nodes() {
        let mut rng = StdRng::seed_from_u64(7);
        let hp = Honeypots::new(&mut rng);
        let frag = hp.generate(5, &mut rng);
        // 5 decoys -> 5 top-level hidden nodes.
        assert_eq!(frag.matches("aria-hidden=\"true\"").count(), 5);
    }

    #[test]
    fn decoys_are_hidden_and_inert() {
        let mut rng = StdRng::seed_from_u64(1);
        let hp = Honeypots::new(&mut rng);
        let frag = hp.generate(10, &mut rng);
        assert!(frag.contains("display:none"));
        assert!(frag.contains("tabindex=\"-1\""));
    }

    #[test]
    fn decoys_and_removal_share_marker() {
        let mut rng = StdRng::seed_from_u64(3);
        let hp = Honeypots::new(&mut rng);
        let frag = hp.generate(5, &mut rng);
        let script = hp.removal_script();
        assert!(frag.contains(&hp.marker), "decoys must carry the marker");
        assert!(script.contains(&hp.marker), "removal script must target the marker");
        assert!(script.contains("e.remove()"));
    }

    #[test]
    fn deterministic_with_seed() {
        let mut a = StdRng::seed_from_u64(42);
        let mut b = StdRng::seed_from_u64(42);
        let ha = Honeypots::new(&mut a);
        let hb = Honeypots::new(&mut b);
        assert_eq!(ha.generate(4, &mut a), hb.generate(4, &mut b));
    }

    #[test]
    fn decoy_block_is_dense_link_free_prose() {
        let mut rng = StdRng::seed_from_u64(5);
        let hp = Honeypots::new(&mut rng);
        let mut out = String::new();
        hp.decoy_block(&mut out, &mut rng);
        // A content-like class extractors reward as main content.
        assert!(CONTENT_CLASSES.iter().any(|c| out.contains(*c)), "{out}");
        // Sentence-length prose in paragraphs, not a short token.
        assert!(out.contains("<p>") && out.contains('.'), "{out}");
        // Link-free, so extractors don't score it as nav/boilerplate.
        assert!(!out.contains("<a "), "{out}");
        // Still hidden and marked for client-side removal.
        assert!(out.contains("display:none") && out.contains(&hp.marker), "{out}");
    }

    #[test]
    fn wordlists_loaded_from_assets() {
        // Asset files parsed at first use; empty would mean a bad path / format.
        assert!(!WORDS.is_empty() && !PROSE_NOUNS.is_empty() && !CONTENT_CLASSES.is_empty());
    }
}
