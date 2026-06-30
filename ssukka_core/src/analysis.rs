use crate::config::ObfuscationConfig;
use crate::css;
use crate::js;
use crate::symbol_map::SymbolMap;
use lol_html::{element, text, HtmlRewriter, Settings};
use std::cell::RefCell;

/// Scan the document and collect every class/ID symbol.
///
/// Streams the HTML with lol_html, gathering class and id attributes, CSS
/// selectors from `<style>`, and DOM API references from `<script>`.
pub fn analyze(html: &str, config: &ObfuscationConfig) -> SymbolMap {
    let mut symbols = SymbolMap::new(config.seed);
    let rename_classes = config.rename_classes;
    let rename_ids = config.rename_ids;

    // lol_html handlers are FnMut, so buffer into RefCells and process after
    // the rewrite finishes.
    let class_names: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let id_names: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let style_contents: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let script_contents: RefCell<Vec<String>> = RefCell::new(Vec::new());

    let in_style: RefCell<bool> = RefCell::new(false);
    let in_script: RefCell<bool> = RefCell::new(false);
    let script_is_js: RefCell<bool> = RefCell::new(true);
    let style_buf: RefCell<String> = RefCell::new(String::new());
    let script_buf: RefCell<String> = RefCell::new(String::new());

    {
        let settings = Settings::new()
            .append_element_content_handler(element!("*", |el| {
                if rename_classes {
                    if let Some(class_attr) = el.get_attribute("class") {
                        for class in class_attr.split_whitespace() {
                            class_names.borrow_mut().push(class.to_owned());
                        }
                    }
                }
                if rename_ids {
                    if let Some(id_attr) = el.get_attribute("id") {
                        id_names.borrow_mut().push(id_attr.to_owned());
                    }
                    // `for` attribute (label-to-input association)
                    if let Some(for_attr) = el.get_attribute("for") {
                        id_names.borrow_mut().push(for_attr.to_owned());
                    }
                }
                Ok(())
            }))
            .append_element_content_handler(element!("style", |_el| {
                *in_style.borrow_mut() = true;
                *style_buf.borrow_mut() = String::new();
                Ok(())
            }))
            // Treat as JS only, not JSON/template
            .append_element_content_handler(element!("script", |el| {
                let is_js = match el.get_attribute("type") {
                    Some(t) => {
                        let t = t.to_ascii_lowercase();
                        t.is_empty() || t.contains("javascript") || t.contains("ecmascript")
                    },
                    None => true,
                };
                *script_is_js.borrow_mut() = is_js;
                *in_script.borrow_mut() = true;
                *script_buf.borrow_mut() = String::new();
                Ok(())
            }))
            .append_element_content_handler(text!("style", |text| {
                style_buf.borrow_mut().push_str(text.as_str());
                if text.last_in_text_node() {
                    let content = style_buf.borrow().clone();
                    if !content.is_empty() {
                        style_contents.borrow_mut().push(content);
                    }
                    *in_style.borrow_mut() = false;
                }
                Ok(())
            }))
            .append_element_content_handler(text!("script", |text| {
                script_buf.borrow_mut().push_str(text.as_str());
                if text.last_in_text_node() {
                    if *script_is_js.borrow() {
                        let content = script_buf.borrow().clone();
                        if !content.is_empty() {
                            script_contents.borrow_mut().push(content);
                        }
                    }
                    *in_script.borrow_mut() = false;
                }
                Ok(())
            }));
        let mut rewriter = HtmlRewriter::new(settings, |_chunk: &[u8]| {});

        rewriter.write(html.as_bytes()).ok();
        rewriter.end().ok();
    }

    for class in class_names.borrow().iter() {
        symbols.register_class(class);
    }
    for id in id_names.borrow().iter() {
        symbols.register_id(id);
    }

    for css_content in style_contents.borrow().iter() {
        css::extract_selectors(css_content, &mut symbols, rename_classes, rename_ids);
    }

    for js_content in script_contents.borrow().iter() {
        js::extract_js_references(js_content, &mut symbols, rename_classes, rename_ids);
    }

    // Collect ID references from href="#id" attributes too.
    if rename_ids {
        collect_href_id_refs(html, &mut symbols);
    }

    // Detect JS concatenation prefixes and resolve compound class names.
    if rename_classes {
        let mut all_prefixes = Vec::new();
        for js_content in script_contents.borrow().iter() {
            let mut prefixes = js::extract_concatenation_prefixes(js_content);
            all_prefixes.append(&mut prefixes);
        }
        symbols.resolve_compounds(&all_prefixes);
    }

    symbols
}

/// Scan for `href="#id"` patterns and register the referenced IDs.
fn collect_href_id_refs(html: &str, symbols: &mut SymbolMap) {
    let id_refs: RefCell<Vec<String>> = RefCell::new(Vec::new());

    {
        let settings = Settings::new().append_element_content_handler(element!("*", |el| {
            // href="#someId"
            if let Some(href) = el.get_attribute("href") {
                if let Some(id) = href.strip_prefix('#') {
                    if !id.is_empty() {
                        id_refs.borrow_mut().push(id.to_owned());
                    }
                }
            }
            // aria-labelledby, aria-describedby, aria-controls, etc.
            for attr_name in &["aria-labelledby", "aria-describedby", "aria-controls", "aria-owns"] {
                if let Some(value) = el.get_attribute(attr_name) {
                    for id in value.split_whitespace() {
                        id_refs.borrow_mut().push(id.to_owned());
                    }
                }
            }
            Ok(())
        }));
        let mut rewriter = HtmlRewriter::new(settings, |_chunk: &[u8]| {});

        rewriter.write(html.as_bytes()).ok();
        rewriter.end().ok();
    }

    for id in id_refs.borrow().iter() {
        symbols.register_id(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> ObfuscationConfig {
        ObfuscationConfig {
            seed: Some(42),
            ..Default::default()
        }
    }

    #[test]
    fn collect_classes_and_ids() {
        let html = r#"<div class="foo bar" id="main"><span class="baz">test</span></div>"#;
        let symbols = analyze(html, &default_config());
        assert!(symbols.get_class("foo").is_some());
        assert!(symbols.get_class("bar").is_some());
        assert!(symbols.get_class("baz").is_some());
        assert!(symbols.get_id("main").is_some());
    }

    #[test]
    fn collect_style_selectors() {
        let html = r#"<style>.container { display: flex; } #header { color: blue; }</style>"#;
        let symbols = analyze(html, &default_config());
        assert!(symbols.get_class("container").is_some());
        assert!(symbols.get_id("header").is_some());
    }

    #[test]
    fn collect_script_references() {
        let html = r#"<script>document.getElementById("app");</script>"#;
        let symbols = analyze(html, &default_config());
        assert!(symbols.get_id("app").is_some());
    }

    #[test]
    fn collect_href_id_refs() {
        let html = r##"<a href="#section1">link</a><div id="section1">content</div>"##;
        let symbols = analyze(html, &default_config());
        assert!(symbols.get_id("section1").is_some());
    }

    #[test]
    fn collect_for_attribute() {
        let html = r#"<label for="email">Email</label><input id="email">"#;
        let symbols = analyze(html, &default_config());
        assert!(symbols.get_id("email").is_some());
    }
}
