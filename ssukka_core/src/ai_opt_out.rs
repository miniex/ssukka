//! Machine-readable AI opt-out / TDM rights-reservation signals.
//!
//! As an HTML rewriter, ssukka emits only the HTML-bindable signals (TDMRep
//! `<meta>`, legacy `noai`, best-effort AIPREF `Content-Usage` via `http-equiv`).
//! The canonical AIPREF/TDMRep transports — an HTTP header, a `robots.txt` rule,
//! and `/.well-known/tdmrep.json` — are exposed as helpers for edge/server use.
//!
//! These are advisory, legally-recognized declarations (EU CDSM Art.4 / AI Act),
//! not a technical barrier — pair with honeypots/structural for real friction.
//!
//! Standards (drafts, treat as versioned): IETF AIPREF (`draft-ietf-aipref-*`),
//! W3C TDMRep (CG Final Report, 2024-05-10).

/// IETF AIPREF vocabulary draft this build targets; bump as the WG advances.
pub const AIPREF_VOCAB_DRAFT: &str = "draft-ietf-aipref-vocab-06";

/// AIPREF `Content-Usage`: opt out of AI *training*, leave `search` (link-back /
/// RAG) allowed — the common publisher default.
pub const CONTENT_USAGE_OPT_OUT: &str = "train-ai=n";

/// AI *training* crawler tokens for `robots.txt` Disallow groups. Moving target —
/// audit periodically. (Source: Cloudflare AIndependence, 2024-25.)
pub const AI_TRAINING_CRAWLERS: &[&str] = &[
    "GPTBot",
    "ClaudeBot",
    "CCBot",
    "Google-Extended",
    "Applebot-Extended",
    "Amazonbot",
    "Bytespider",
    "PerplexityBot",
    "Meta-ExternalAgent",
];

/// The `<meta>` block prepended to `<head>`: legacy `noai`, TDMRep
/// `tdm-reservation` (+ optional `tdm-policy`), and best-effort AIPREF
/// `Content-Usage` (canonical transport is the HTTP header / robots.txt).
pub fn meta_block(tdm_policy: Option<&str>) -> String {
    let mut s = String::with_capacity(220);
    s.push_str(r#"<meta name="robots" content="noai, noimageai">"#);
    s.push_str(r#"<meta name="tdm-reservation" content="1">"#);
    if let Some(url) = tdm_policy {
        s.push_str(r#"<meta name="tdm-policy" content=""#);
        push_attr_escaped(&mut s, url);
        s.push_str(r#"">"#);
    }
    s.push_str(r#"<meta http-equiv="Content-Usage" content=""#);
    s.push_str(CONTENT_USAGE_OPT_OUT);
    s.push_str(r#"">"#);
    s
}

/// Ready-to-serve `robots.txt`: a site-wide AIPREF `Content-Usage` opt-out rule
/// plus a `Disallow` group per [`AI_TRAINING_CRAWLERS`] token. Edge/server only.
pub fn robots_txt() -> String {
    let mut s = String::with_capacity(64 + AI_TRAINING_CRAWLERS.len() * 32);
    s.push_str("User-agent: *\n");
    s.push_str("Content-Usage: ");
    s.push_str(CONTENT_USAGE_OPT_OUT);
    s.push_str("\n\n");
    for bot in AI_TRAINING_CRAWLERS {
        s.push_str("User-agent: ");
        s.push_str(bot);
        s.push_str("\nDisallow: /\n\n");
    }
    s
}

/// The AIPREF `Content-Usage` HTTP response-header value, to stamp at the edge.
pub fn content_usage_header() -> &'static str {
    CONTENT_USAGE_OPT_OUT
}

/// The `/.well-known/tdmrep.json` body declaring a site-wide TDM reservation
/// (TDMRep). `tdm-policy` (a URL to the licensing policy) is included when given.
pub fn well_known_tdmrep_json(tdm_policy: Option<&str>) -> String {
    match tdm_policy {
        Some(url) => {
            let mut s = String::with_capacity(48 + url.len());
            s.push_str(r#"[{"location":"/","tdm-reservation":1,"tdm-policy":""#);
            push_json_escaped(&mut s, url);
            s.push_str(r#""}]"#);
            s
        },
        None => r#"[{"location":"/","tdm-reservation":1}]"#.to_string(),
    }
}

fn push_attr_escaped(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
}

fn push_json_escaped(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_block_carries_all_three_signals() {
        let m = meta_block(None);
        // legacy noai
        assert!(m.contains(r#"<meta name="robots" content="noai, noimageai">"#));
        // TDMRep reservation
        assert!(m.contains(r#"<meta name="tdm-reservation" content="1">"#));
        // AIPREF Content-Usage (best-effort http-equiv)
        assert!(m.contains(r#"<meta http-equiv="Content-Usage" content="train-ai=n">"#));
        // no policy meta when none given
        assert!(!m.contains("tdm-policy"));
    }

    #[test]
    fn meta_block_includes_escaped_policy_when_given() {
        let m = meta_block(Some("https://ex.com/p?a=1&b=2"));
        assert!(m.contains(r#"<meta name="tdm-policy" content="https://ex.com/p?a=1&amp;b=2">"#));
    }

    #[test]
    fn robots_txt_has_content_usage_and_per_bot_disallow() {
        let r = robots_txt();
        assert!(r.contains("Content-Usage: train-ai=n"));
        assert!(r.contains("User-agent: GPTBot\nDisallow: /"));
        assert!(r.contains("User-agent: Google-Extended\nDisallow: /"));
        // one Disallow per known training crawler
        assert_eq!(r.matches("Disallow: /").count(), AI_TRAINING_CRAWLERS.len());
    }

    #[test]
    fn tdmrep_json_with_and_without_policy() {
        assert_eq!(
            well_known_tdmrep_json(None),
            r#"[{"location":"/","tdm-reservation":1}]"#
        );
        assert_eq!(
            well_known_tdmrep_json(Some("https://ex.com/policy")),
            r#"[{"location":"/","tdm-reservation":1,"tdm-policy":"https://ex.com/policy"}]"#
        );
    }

    #[test]
    fn content_usage_header_value() {
        assert_eq!(content_usage_header(), "train-ai=n");
    }
}
