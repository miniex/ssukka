use crate::config::ObfuscationConfig;
use crate::css;
use crate::error::Result;
use crate::html::entities;
use crate::html::tags;
use crate::html::whitespace;
use crate::js;
use crate::symbol_map::SymbolMap;
use lol_html::html_content::ContentType;
use lol_html::{doc_comments, element, text, HtmlRewriter, Settings};
use rand::rngs::StdRng;
use rand::SeedableRng;
use std::cell::RefCell;
use std::rc::Rc;

/// ID-referencing attributes that need consistent renaming.
const ID_REF_ATTRS: &[&str] = &[
    "for",
    "aria-labelledby",
    "aria-describedby",
    "aria-controls",
    "aria-owns",
    "aria-activedescendant",
    "aria-flowto",
    "form",
    "headers",
    "list",
    "popovertarget",
];

/// Attributes that should not be entity-encoded (URLs, IDs, etc.)
fn should_skip_attr_encoding(name: &str) -> bool {
    matches!(
        name,
        "class" | "id" | "src" | "href" | "action" | "style" | "type" | "name" | "value"
    ) || ID_REF_ATTRS.contains(&name)
}

/// Pass 2: Apply all obfuscation transformations.
pub fn transform(html: &str, symbols: &SymbolMap, config: &ObfuscationConfig) -> Result<String> {
    let output = RefCell::new(Vec::with_capacity(html.len()));
    let rng = RefCell::new(match config.seed {
        Some(s) => StdRng::seed_from_u64(s),
        None => StdRng::from_rng(&mut rand::rng()),
    });

    let remove_comments = config.remove_comments;
    let collapse_ws = config.collapse_whitespace;
    let encode_text = config.encode_text_entities;
    let encode_attrs = config.encode_attr_entities;
    let shuffle_attrs = config.shuffle_attributes;
    let randomize_case = config.randomize_tag_case;
    let rename_classes = config.rename_classes;
    let rename_ids = config.rename_ids;
    let minify_css = config.minify_css;
    let unicode_escape = config.unicode_escape_selectors;
    let encode_js = config.encode_js_strings;
    let minify_js_opt = config.minify_js;

    // Track preserved-whitespace context (Rc for sharing with end_tag_handlers)
    let preserved_depth = Rc::new(RefCell::new(0u32));

    // Track whether we're inside a <style> or <script> RAWTEXT element.
    // Text inside these must NOT be entity-encoded — browsers parse them as raw text.
    let in_raw_text = Rc::new(RefCell::new(false));

    // Track whether the current <script> is actual JavaScript (not JSON, template, etc.)
    let script_is_js = RefCell::new(true);

    // Accumulation buffers for style/script text (lol_html may split text chunks)
    let style_buf: RefCell<String> = RefCell::new(String::new());
    let script_buf: RefCell<String> = RefCell::new(String::new());

    let mut element_handlers: Vec<_> = Vec::new();
    let mut document_handlers: Vec<_> = Vec::new();

    // Comment removal (document-level)
    if remove_comments {
        document_handlers.push(doc_comments!(|comment| {
            comment.remove();
            Ok(())
        }));
    }

    // Track entering/leaving style elements
    element_handlers.push(element!("style", |el| {
        *style_buf.borrow_mut() = String::new();
        *in_raw_text.borrow_mut() = true;
        if let Some(handlers) = el.end_tag_handlers() {
            let flag = Rc::clone(&in_raw_text);
            let handler: lol_html::EndTagHandler<'static> = Box::new(move |_end| {
                *flag.borrow_mut() = false;
                Ok(())
            });
            handlers.push(handler);
        }
        Ok(())
    }));

    // Track entering/leaving script elements
    element_handlers.push(element!("script", |el| {
        *script_buf.borrow_mut() = String::new();
        *in_raw_text.borrow_mut() = true;

        // Check if this is actual JavaScript (no type, or type containing "javascript")
        let is_js = match el.get_attribute("type") {
            Some(t) => {
                let t = t.to_ascii_lowercase();
                t.is_empty() || t.contains("javascript") || t.contains("ecmascript")
            }
            None => true,
        };
        *script_is_js.borrow_mut() = is_js;

        if let Some(handlers) = el.end_tag_handlers() {
            let flag = Rc::clone(&in_raw_text);
            let handler: lol_html::EndTagHandler<'static> = Box::new(move |_end| {
                *flag.borrow_mut() = false;
                Ok(())
            });
            handlers.push(handler);
        }
        Ok(())
    }));

    // Element handler for attributes, tag case, class/ID renaming
    element_handlers.push(element!("*", |el| {
        let tag_lower = el.tag_name().to_ascii_lowercase();

        // Track preserved whitespace elements
        if whitespace::is_preserved_tag(&tag_lower) {
            *preserved_depth.borrow_mut() += 1;
            if let Some(handlers) = el.end_tag_handlers() {
                let depth = Rc::clone(&preserved_depth);
                let handler: lol_html::EndTagHandler<'static> = Box::new(move |_end| {
                    let mut d = depth.borrow_mut();
                    if *d > 0 {
                        *d -= 1;
                    }
                    Ok(())
                });
                handlers.push(handler);
            }
        }

        // Rename class attribute (applies to all elements including style/script)
        if rename_classes {
            if let Some(class_attr) = el.get_attribute("class") {
                let new_classes: Vec<&str> = class_attr
                    .split_whitespace()
                    .map(|c| symbols.get_class(c).unwrap_or(c))
                    .collect();
                el.set_attribute("class", &new_classes.join(" "))?;
            }
        }

        // Rename id attribute (applies to all elements including style/script)
        if rename_ids {
            if let Some(id_attr) = el.get_attribute("id") {
                if let Some(new_id) = symbols.get_id(&id_attr) {
                    el.set_attribute("id", new_id)?;
                }
            }

            // Rename ID-referencing attributes
            for &attr_name in ID_REF_ATTRS {
                if let Some(value) = el.get_attribute(attr_name) {
                    let new_value: Vec<&str> = value
                        .split_whitespace()
                        .map(|id| symbols.get_id(id).unwrap_or(id))
                        .collect();
                    el.set_attribute(attr_name, &new_value.join(" "))?;
                }
            }

            // Handle href="#id"
            if let Some(href) = el.get_attribute("href") {
                if let Some(id) = href.strip_prefix('#') {
                    if let Some(new_id) = symbols.get_id(id) {
                        el.set_attribute("href", &format!("#{new_id}"))?;
                    }
                }
            }
        }

        // Skip style/script for further obfuscation (entity encoding, shuffle, case)
        if tag_lower == "style" || tag_lower == "script" {
            return Ok(());
        }

        // Encode attribute values (skip functional attrs like IDs, URLs)
        if encode_attrs {
            let mut rng = rng.borrow_mut();
            let attrs: Vec<(String, String)> = el
                .attributes()
                .iter()
                .map(|a| (a.name(), a.value()))
                .collect();
            for (name, value) in &attrs {
                if should_skip_attr_encoding(name) {
                    continue;
                }
                let encoded = entities::encode_attr_value(value, &mut rng);
                el.set_attribute(name, &encoded)?;
            }
        }

        // Shuffle attributes
        if shuffle_attrs {
            let mut rng = rng.borrow_mut();
            let mut attrs: Vec<(String, String)> = el
                .attributes()
                .iter()
                .map(|a| (a.name(), a.value()))
                .collect();
            tags::shuffle_attributes(&mut attrs, &mut rng);

            let attr_names: Vec<String> = el.attributes().iter().map(|a| a.name()).collect();
            for name in &attr_names {
                el.remove_attribute(name);
            }
            for (name, value) in &attrs {
                el.set_attribute(name, value)?;
            }
        }

        // Randomize tag case
        if randomize_case {
            let mut rng = rng.borrow_mut();
            let new_tag = tags::randomize_tag_case(&tag_lower, &mut rng);
            el.set_tag_name(&new_tag)?;
        }

        Ok(())
    }));

    // General text handler — applies to ALL text nodes, but skips RAWTEXT context
    element_handlers.push(text!("*", |text| {
        // Skip text inside <style> and <script> — handled by dedicated handlers
        if *in_raw_text.borrow() {
            return Ok(());
        }

        let is_preserved = *preserved_depth.borrow() > 0;
        let content = text.as_str().to_owned();

        if content.is_empty() {
            return Ok(());
        }

        let mut processed = content;

        if collapse_ws && !is_preserved {
            processed = whitespace::collapse_whitespace(&processed);
        }

        if encode_text {
            let mut rng = rng.borrow_mut();
            processed = entities::encode_entities(&processed, &mut rng);
        }

        text.replace(&processed, ContentType::Html);
        Ok(())
    }));

    // Style content handler — accumulate chunks, process on last
    element_handlers.push(text!("style", |text| {
        style_buf.borrow_mut().push_str(text.as_str());

        if text.last_in_text_node() {
            let css_text = style_buf.borrow().clone();
            if !css_text.is_empty() {
                if let Ok(transformed) = css::transform_css(
                    &css_text,
                    symbols,
                    minify_css,
                    unicode_escape,
                    rename_classes,
                    rename_ids,
                ) {
                    text.replace(&transformed, ContentType::Html);
                }
            }
            *style_buf.borrow_mut() = String::new();
        } else {
            text.remove();
        }
        Ok(())
    }));

    // Script content handler — accumulate chunks, process on last
    element_handlers.push(text!("script", |text| {
        script_buf.borrow_mut().push_str(text.as_str());

        if text.last_in_text_node() {
            let content = script_buf.borrow().clone();
            if !content.is_empty() {
                if *script_is_js.borrow() {
                    // JavaScript: full transformation
                    let transformed = js::transform_js(
                        &content,
                        symbols,
                        encode_js,
                        minify_js_opt,
                        rename_classes,
                        rename_ids,
                    );
                    text.replace(&transformed, ContentType::Html);
                } else if rename_classes || rename_ids {
                    // Non-JS (JSON, etc.): only rename class/ID references
                    let transformed = js::replace_symbols_word_boundary(
                        &content,
                        symbols,
                        rename_classes,
                        rename_ids,
                    );
                    text.replace(&transformed, ContentType::Html);
                }
            }
            *script_buf.borrow_mut() = String::new();
        } else {
            text.remove();
        }
        Ok(())
    }));

    {
        let mut rewriter = HtmlRewriter::new(
            Settings {
                element_content_handlers: element_handlers,
                document_content_handlers: document_handlers,
                ..Settings::default()
            },
            |chunk: &[u8]| {
                output.borrow_mut().extend_from_slice(chunk);
            },
        );

        rewriter.write(html.as_bytes())?;
        rewriter.end()?;
    }

    let bytes = output.into_inner();
    String::from_utf8(bytes).map_err(|e| crate::error::SsukkaError::Rewrite(e.to_string()))
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
    fn removes_comments() {
        let html = "<div><!-- comment -->text</div>";
        let symbols = SymbolMap::new(Some(42));
        let result = transform(html, &symbols, &default_config()).unwrap();
        assert!(!result.contains("comment"));
    }

    #[test]
    fn preserves_pre_whitespace() {
        let html = "<pre>  code  \n  here  </pre>";
        let config = ObfuscationConfig {
            seed: Some(42),
            encode_text_entities: false,
            randomize_tag_case: false,
            shuffle_attributes: false,
            ..Default::default()
        };
        let symbols = SymbolMap::new(Some(42));
        let result = transform(html, &symbols, &config).unwrap();
        assert!(result.contains("  code  \n  here  "));
    }

    #[test]
    fn renames_classes_consistently() {
        let html = r#"<div class="foo">text</div>"#;
        let config = default_config();
        let mut symbols = SymbolMap::new(Some(42));
        symbols.register_class("foo");
        let obf = symbols.get_class("foo").unwrap().to_owned();
        let result = transform(html, &symbols, &config).unwrap();
        assert!(result.contains(&obf));
    }

    #[test]
    fn renames_href_id_refs() {
        let html = r##"<a href="#sec">link</a><div id="sec">content</div>"##;
        let config = default_config();
        let mut symbols = SymbolMap::new(Some(42));
        symbols.register_id("sec");
        let obf = symbols.get_id("sec").unwrap().to_owned();
        let result = transform(html, &symbols, &config).unwrap();
        assert!(result.contains(&format!("#{obf}")));
    }

    #[test]
    fn style_not_entity_encoded() {
        let html = "<style>.foo { color: red; }</style>";
        let config = ObfuscationConfig {
            seed: Some(42),
            rename_classes: false,
            minify_css: false,
            unicode_escape_selectors: false,
            randomize_tag_case: false,
            shuffle_attributes: false,
            ..Default::default()
        };
        let symbols = SymbolMap::new(Some(42));
        let result = transform(html, &symbols, &config).unwrap();
        // CSS content should NOT contain HTML entities
        assert!(
            !result.contains("&#"),
            "Style content should not be entity-encoded. Got: {result}"
        );
        assert!(result.contains("color"));
    }

    #[test]
    fn script_not_entity_encoded() {
        let html = r#"<script>var x = 1 + 2;</script>"#;
        let config = ObfuscationConfig {
            seed: Some(42),
            encode_js_strings: false,
            minify_js: false,
            randomize_tag_case: false,
            shuffle_attributes: false,
            ..Default::default()
        };
        let symbols = SymbolMap::new(Some(42));
        let result = transform(html, &symbols, &config).unwrap();
        assert!(
            result.contains("var x = 1 + 2"),
            "Script content should not be entity-encoded. Got: {result}"
        );
    }
}
