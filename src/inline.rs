//! Optional inlining of local `<link rel=stylesheet>` and `<script src>` as a
//! preprocessing step, so the inlined CSS/JS is then obfuscated normally. Only
//! files resolving under `base_dir` are inlined; URLs, protocol-relative refs,
//! `data:` URIs, and paths escaping `base_dir` are left untouched. Never hits
//! the network.

use lol_html::html_content::ContentType;
use lol_html::{element, rewrite_str, Settings};
use std::path::{Path, PathBuf};

/// Inline local stylesheet/script references in `html`.
///
/// `base_dir` defaults to the current directory when `None`. On any rewrite
/// error the original HTML is returned unchanged.
pub fn inline_local(html: &str, base_dir: Option<&Path>) -> String {
    let base = base_dir.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let canon_base = base.canonicalize().ok();

    let (b1, cb1) = (base.clone(), canon_base.clone());
    let (b2, cb2) = (base.clone(), canon_base.clone());

    let settings = Settings::new()
        .append_element_content_handler(element!("link[rel]", move |el| {
            let rel = el.get_attribute("rel").unwrap_or_default().to_ascii_lowercase();
            if !rel.split_whitespace().any(|r| r == "stylesheet") {
                return Ok(());
            }
            if let Some(href) = el.get_attribute("href") {
                if let Some(css) = resolve_local(&href, &b1, cb1.as_deref()) {
                    el.replace(&format!("<style>{css}</style>"), ContentType::Html);
                }
            }
            Ok(())
        }))
        .append_element_content_handler(element!("script[src]", move |el| {
            if let Some(src) = el.get_attribute("src") {
                if let Some(js) = resolve_local(&src, &b2, cb2.as_deref()) {
                    // `</script>` inside the file would break the inline tag.
                    if !js.to_ascii_lowercase().contains("</script") {
                        el.replace(&format!("<script>{js}</script>"), ContentType::Html);
                    }
                }
            }
            Ok(())
        }));

    rewrite_str(html, settings).unwrap_or_else(|_| html.to_owned())
}

/// Read a local file referenced by `href`, or `None` if it is not a safe local
/// path under `base`.
fn resolve_local(href: &str, base: &Path, canon_base: Option<&Path>) -> Option<String> {
    // Reject anything that is not a plain relative/absolute local path.
    if href.is_empty()
        || href.contains("://")
        || href.starts_with("//")
        || href.starts_with('#')
        || href.starts_with("data:")
    {
        return None;
    }
    // Drop any query string / fragment.
    let clean = href.split(['?', '#']).next().unwrap_or(href);
    let canon = base.join(clean).canonicalize().ok()?;
    // Containment check: the resolved file must stay under base_dir.
    if let Some(cb) = canon_base {
        if !canon.starts_with(cb) {
            return None;
        }
    }
    std::fs::read_to_string(&canon).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inlines_local_css_and_js_but_not_urls() {
        let dir = std::env::temp_dir().join("ssukka_inline_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.css"), ".x{color:red}").unwrap();
        std::fs::write(dir.join("a.js"), "var y=1;").unwrap();

        let html = r#"<link rel="stylesheet" href="a.css"><script src="a.js"></script><link rel="stylesheet" href="https://cdn.example/x.css">"#;
        let out = inline_local(html, Some(&dir));

        assert!(out.contains("<style>.x{color:red}</style>"));
        assert!(out.contains("<script>var y=1;</script>"));
        // Remote URL is left as a <link>, never fetched.
        assert!(out.contains("https://cdn.example/x.css"));
        assert!(!out.contains("href=\"a.css\""));
    }

    #[test]
    fn rejects_path_escaping_base_dir() {
        let dir = std::env::temp_dir().join("ssukka_inline_escape");
        std::fs::create_dir_all(&dir).unwrap();
        let html = r#"<link rel="stylesheet" href="../../../etc/hostname">"#;
        let out = inline_local(html, Some(&dir));
        // Untouched: the traversal target is outside base_dir.
        assert!(out.contains(r#"href="../../../etc/hostname""#));
        assert!(!out.contains("<style>"));
    }
}
