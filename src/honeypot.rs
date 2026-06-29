//! Honeypot / decoy injection: invisible trap links, fake form fields, and
//! bogus data blocks that waste scraper effort and poison harvested data.
//! Decoys are hidden from layout and assistive tech, so real users, keyboard
//! nav, and screen readers are unaffected.
//!
//! Each decoy carries a random marker; an injected script removes them on load,
//! so JS clients (real users, JS-rendering scrapers) get a clean DOM with no
//! signature, while no-JS bulk crawlers still ingest the decoys and take the bait.

use rand::rngs::StdRng;
use rand::RngExt;

/// Realistic-looking name fragments for decoy classes / fields.
const WORDS: &[&str] = &[
    "wrapper",
    "inner",
    "content",
    "item",
    "node",
    "list",
    "entry",
    "field",
    "block",
    "panel",
    "row",
    "col",
    "data",
    "meta",
    "label",
    "value",
    "group",
    "section",
    "module",
    "unit",
    "container",
    "holder",
    "box",
    "cell",
];

/// Plausible-looking trap path segments for decoy links.
const PATHS: &[&str] = &[
    "track", "redirect", "collect", "api", "internal", "gateway", "sink", "trap", "beacon", "pixel", "log", "event",
    "session", "ref",
];

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
        let mut out = String::with_capacity(count * 80);
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
        let path = PATHS[rng.random_range(0..PATHS.len())];
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

    /// A decoy data block: realistic class names wrapping fake values to pollute
    /// content scrapers that key off structure.
    fn decoy_block(&self, out: &mut String, rng: &mut StdRng) {
        let cls = format!("{}-{}-{}", word(rng), word(rng), rand_token(rng, 3));
        let val_len = rng.random_range(6..=14);
        let val = rand_token(rng, val_len);
        out.push_str(&format!(
            r#"<div class="{cls}" {h}><span class="{c2}">{val}</span></div>"#,
            h = self.hidden(),
            c2 = word(rng),
        ));
    }
}

fn word(rng: &mut StdRng) -> &'static str {
    WORDS[rng.random_range(0..WORDS.len())]
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
}
