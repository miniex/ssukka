//! Honeypot / decoy injection: invisible trap links, fake form fields, and
//! bogus data blocks that waste scraper effort and poison harvested data.
//! Decoys are hidden from layout and assistive tech, so real users, keyboard
//! nav, and screen readers are unaffected.

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

/// Build a block of invisible decoy nodes.
///
/// Returns an HTML fragment (already escaped) suitable for appending to the
/// document body. The fragment is inserted verbatim into the output and is not
/// re-processed by the rewriter, so its decoy names stay literal on purpose.
pub fn generate(count: usize, rng: &mut StdRng) -> String {
    let mut out = String::with_capacity(count * 80);
    for _ in 0..count {
        match rng.random_range(0u8..3) {
            0 => decoy_link(&mut out, rng),
            1 => decoy_field(&mut out, rng),
            _ => decoy_block(&mut out, rng),
        }
    }
    out
}

/// Inert styling that hides a node from layout while keeping it in the DOM
/// (so naive crawlers that scan the DOM/hrefs still take the bait).
fn hidden_attrs() -> &'static str {
    r#"style="display:none!important" aria-hidden="true" tabindex="-1""#
}

/// A trap link: crawlers that follow every href hit a bogus internal path.
fn decoy_link(out: &mut String, rng: &mut StdRng) {
    let path = PATHS[rng.random_range(0..PATHS.len())];
    let token = rand_token(rng, 8);
    let text = format!("{} {}", word(rng), word(rng));
    out.push_str(&format!(
        r#"<a href="/{path}/{token}" {h} rel="nofollow">{text}</a>"#,
        h = hidden_attrs(),
    ));
}

/// A honeypot form field: spam bots that auto-fill every input reveal themselves.
fn decoy_field(out: &mut String, rng: &mut StdRng) {
    let name = format!("{}_{}", word(rng), word(rng));
    out.push_str(&format!(
        r#"<input type="text" name="{name}" {h} autocomplete="off" value="">"#,
        h = hidden_attrs(),
    ));
}

/// A decoy data block: realistic class names wrapping fake values to pollute
/// content scrapers that key off structure.
fn decoy_block(out: &mut String, rng: &mut StdRng) {
    let cls = format!("{}-{}-{}", word(rng), word(rng), rand_token(rng, 3));
    let val_len = rng.random_range(6..=14);
    let val = rand_token(rng, val_len);
    out.push_str(&format!(
        r#"<div class="{cls}" {h}><span class="{c2}">{val}</span></div>"#,
        h = hidden_attrs(),
        c2 = word(rng),
    ));
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
        let frag = generate(5, &mut rng);
        // 5 decoys -> 5 top-level hidden nodes.
        assert_eq!(frag.matches("aria-hidden=\"true\"").count(), 5);
    }

    #[test]
    fn decoys_are_hidden_and_inert() {
        let mut rng = StdRng::seed_from_u64(1);
        let frag = generate(10, &mut rng);
        assert!(frag.contains("display:none"));
        assert!(frag.contains("tabindex=\"-1\""));
    }

    #[test]
    fn deterministic_with_seed() {
        let mut a = StdRng::seed_from_u64(42);
        let mut b = StdRng::seed_from_u64(42);
        assert_eq!(generate(4, &mut a), generate(4, &mut b));
    }
}
