//! Differential extraction harness: measures how much each transform reduces
//! what a *non-JS* scraper reads, so resilience is evidence-based not asserted.
//!
//! The extractor models the trafilatura / BeautifulSoup tier (what bulk AI
//! crawlers ~use): drop `<script>`/`<style>`/comments, strip tags, decode
//! entities, tokenize. `token_recall` is the fraction of original content words
//! still recovered - low means hidden, high means the transform is only friction.
//! The stronger readability tier (visibility-aware) lives in `tests/extraction.rs`.
//!
//! This is the *efficacy* arm; *fidelity* (a JS client still renders the
//! original) is proven by the Node-execution tests, so low recall != "broken".

use ssukka_core::Obfuscator;
use std::collections::HashSet;

const ARTICLE: &str = r#"<!DOCTYPE html>
<html><head><title>Quarterly Report</title><style>.x{color:red}</style></head>
<body>
<article class="content">
<h1>Acme Corporation Quarterly Earnings</h1>
<p>Revenue reached substantial figures this quarter, driven by international expansion and several new product launches across emerging markets.</p>
<p>The board approved a dividend payable to every shareholder of record before the announced deadline.</p>
</article>
<script>console.log("tracking");</script>
</body></html>"#;

/// Content word tokens a non-JS DOM-aware scraper would recover.
fn naive_text(html: &str) -> HashSet<String> {
    let mut s = remove_blocks(html, "script");
    s = remove_blocks(&s, "style");
    // Drop comments. The DOM concatenates the text on either side with no gap
    // (so `a<!---->b` is one word "ab"), so splice them out with no space.
    while let Some(start) = s.find("<!--") {
        match s[start..].find("-->") {
            Some(end) => s.replace_range(start..start + end + 3, ""),
            None => break,
        }
    }
    let mut text = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                text.push(' ');
            },
            _ if !in_tag => text.push(c),
            _ => {},
        }
    }
    decode_entities(&text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3)
        .map(str::to_lowercase)
        .collect()
}

/// Remove `<tag ...>...</tag>` blocks (case-insensitive; tags are ASCII).
fn remove_blocks(s: &str, tag: &str) -> String {
    let (open, close) = (format!("<{tag}"), format!("</{tag}>"));
    let mut out = s.to_string();
    loop {
        let lower = out.to_ascii_lowercase();
        let Some(start) = lower.find(&open) else { break };
        let Some(rel) = lower[start..].find(&close) else { break };
        out.replace_range(start..start + rel + close.len(), " ");
    }
    out
}

/// Decode the entity forms ssukka emits (decimal, hex, and the named five).
fn decode_entities(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&' {
            if let Some(semi) = chars[i..].iter().position(|&c| c == ';') {
                let entity: String = chars[i + 1..i + semi].iter().collect();
                if let Some(ch) = decode_one(&entity) {
                    out.push(ch);
                    i += semi + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn decode_one(e: &str) -> Option<char> {
    if let Some(hex) = e.strip_prefix("#x").or_else(|| e.strip_prefix("#X")) {
        return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
    }
    if let Some(dec) = e.strip_prefix('#') {
        return dec.parse::<u32>().ok().and_then(char::from_u32);
    }
    match e {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{00a0}'),
        _ => None,
    }
}

/// Fraction of `reference` tokens still present in `candidate`.
fn token_recall(reference: &HashSet<String>, candidate: &HashSet<String>) -> f64 {
    if reference.is_empty() {
        return 1.0;
    }
    let hit = reference.iter().filter(|t| candidate.contains(*t)).count();
    hit as f64 / reference.len() as f64
}

#[test]
fn structural_obfuscation_starves_non_js_extraction() {
    let reference = naive_text(ARTICLE);
    let obf = Obfuscator::builder()
        .seed(1)
        .structural_obfuscation(true)
        .build()
        .obfuscate(ARTICLE)
        .unwrap();
    let recall = token_recall(&reference, &naive_text(&obf));
    println!("structural: non-JS extractor recall = {recall:.2} (lower is better)");
    // Body content is moved into encoded attributes; only non-flow text (the
    // <title>) survives, so a no-JS scraper recovers almost nothing.
    assert!(
        recall < 0.3,
        "structural should starve naive extraction, got {recall:.2}"
    );
}

#[test]
fn cosmetic_default_is_only_friction() {
    // Honest baseline: the default cosmetic transforms (entity encoding, class
    // renaming, ...) do NOT hide text from a DOM-aware decoder - it recovers it.
    let reference = naive_text(ARTICLE);
    let obf = Obfuscator::builder().seed(1).build().obfuscate(ARTICLE).unwrap();
    let recall = token_recall(&reference, &naive_text(&obf));
    println!("cosmetic default: non-JS extractor recall = {recall:.2} (friction only)");
    assert!(
        recall > 0.8,
        "cosmetic obfuscation is friction, text recovers, got {recall:.2}"
    );
}

#[test]
fn comment_split_breaks_substring_search_only() {
    let obf = Obfuscator::builder()
        .seed(1)
        .split_words(true)
        .encode_text_entities(false)
        .build()
        .obfuscate(ARTICLE)
        .unwrap();
    // A keyword search over the raw HTML fails (the word is fragmented)...
    assert!(
        !obf.contains("shareholder"),
        "comment-split should fragment words in raw HTML"
    );
    // ...but a comment-dropping DOM extractor still recovers it. So its reach is
    // naive substring/regex scrapers, not DOM-aware ones - measured, not assumed.
    let recall = token_recall(&naive_text(ARTICLE), &naive_text(&obf));
    println!("comment-split: substring-hidden, DOM recall = {recall:.2}");
    assert!(recall > 0.8, "comment-split does not stop a comment-dropping extractor");
}

#[test]
fn structural_plus_honeypots_poison_naive_extraction() {
    // --structural moves the real body into data-attrs; the display:none decoy
    // blocks survive a naive, visibility-blind scraper. So it harvests filler, not
    // the article - recall collapses while it still ingests a blob of decoy tokens.
    // (A readability-tier extractor drops the hidden decoys; see extraction.rs.)
    let reference = naive_text(ARTICLE);
    let obf = Obfuscator::builder()
        .seed(1)
        .structural_obfuscation(true)
        .inject_honeypots(true)
        .honeypot_count(10)
        .build()
        .obfuscate(ARTICLE)
        .unwrap();
    let got = naive_text(&obf);
    let recall = token_recall(&reference, &got);
    println!(
        "poison: real-body recall = {recall:.2}, harvested tokens = {}",
        got.len()
    );
    // Real body starved (only the non-flow <title> survives).
    assert!(recall < 0.3, "real body should be starved, got {recall:.2}");
    // ...yet the scraper still walks away with a pile of decoy filler.
    assert!(got.len() >= 10, "decoys should supply filler tokens, got {}", got.len());
}
