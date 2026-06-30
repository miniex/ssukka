//! Real-extractor efficacy: runs a readability-tier extractor (`dom_smoothie`, a
//! Mozilla Readability port) before/after obfuscation - the visibility-aware tier
//! above the naive harness in `efficacy.rs`. Dev-dependency only.

use dom_smoothie::Readability;
use ssukka_core::Obfuscator;
use std::collections::HashSet;

const ARTICLE: &str = r#"<!DOCTYPE html>
<html><head><title>Quarterly Earnings</title></head>
<body>
<nav><a href="/">Home</a><a href="/news">News</a></nav>
<article>
<h1>Acme Corporation Quarterly Earnings</h1>
<p>Revenue reached substantial figures this quarter, driven by international expansion and several new product launches across emerging markets that analysts had not anticipated this year.</p>
<p>The board approved a dividend payable to every shareholder of record before the announced deadline, citing strong cash reserves and a disciplined approach to operating expenditure.</p>
<p>Management highlighted that subscription growth in the enterprise segment outpaced consumer hardware for the third consecutive period, reshaping the long term revenue mix considerably.</p>
<p>Looking ahead, the company guided toward continued margin improvement while warning that currency volatility in overseas territories could temper headline growth in coming quarters.</p>
<p>Independent commentators noted that the cautious guidance reflected broader uncertainty across the technology sector rather than any company specific weakness or operational shortfall.</p>
</article>
<footer><p>Copyright Acme Corporation</p></footer>
</body></html>"#;

fn tokens(s: &str) -> HashSet<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3)
        .map(str::to_lowercase)
        .collect()
}

/// Plain text a readability-tier extractor recovers, or "" if it bails.
fn extract(html: &str) -> String {
    match Readability::new(html, None, None).and_then(|mut r| r.parse()) {
        Ok(a) => a.text_content.to_string(),
        Err(_) => String::new(),
    }
}

fn recall(reference: &HashSet<String>, got: &HashSet<String>) -> f64 {
    if reference.is_empty() {
        return 1.0;
    }
    let hit = reference.iter().filter(|t| got.contains(*t)).count();
    hit as f64 / reference.len() as f64
}

/// Body tokens the extractor recovers from the clean article (the baseline).
fn reference() -> HashSet<String> {
    tokens(&extract(ARTICLE))
}

#[test]
fn readability_recovers_clean_article() {
    let r = reference();
    // The extractor pulls the body and drops nav/footer boilerplate.
    assert!(
        r.len() >= 50,
        "extractor should recover the body, got {} tokens",
        r.len()
    );
    for w in ["revenue", "shareholder", "dividend", "subscription", "guidance"] {
        assert!(r.contains(w), "missing distinctive body word: {w}");
    }
    assert!(!r.contains("copyright"), "footer boilerplate should be dropped");
}

#[test]
fn structural_starves_readability_extractor() {
    let obf = Obfuscator::builder()
        .seed(1)
        .structural_obfuscation(true)
        .build()
        .obfuscate(ARTICLE)
        .unwrap();
    let r = recall(&reference(), &tokens(&extract(&obf)));
    assert!(
        r < 0.1,
        "structural should starve the readability extractor, got {r:.2}"
    );
}

#[test]
fn readability_tier_ignores_hidden_decoys() {
    // Real body starved by --structural; decoys are display:none + aria-hidden, so
    // a readability-tier extractor drops them too - poisoning is a naive-tier effect.
    let obf = Obfuscator::builder()
        .seed(1)
        .structural_obfuscation(true)
        .inject_honeypots(true)
        .honeypot_count(8)
        .build()
        .obfuscate(ARTICLE)
        .unwrap();
    let got = extract(&obf);
    assert!(
        recall(&reference(), &tokens(&got)) < 0.1,
        "real body must stay starved even with decoys present"
    );
    assert!(
        got.len() < 80,
        "readability tier should not harvest the hidden decoy prose, got {} chars",
        got.len()
    );
}
