use crate::config::{JsStringEncoding, ObfuscationConfig};
use crate::css;
use crate::error::Result;
use crate::honeypot;
use crate::html::entities;
use crate::html::tags;
use crate::html::whitespace;
use crate::js;
use crate::js_ast;
use crate::structural;
use crate::symbol_map::SymbolMap;
use crate::watermark;
use crate::word_split;
use lol_html::html_content::ContentType;
use lol_html::{doc_comments, element, text, HtmlRewriter, Settings};
use rand::rngs::StdRng;
use rand::RngExt;
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

/// Attributes that must not be entity-encoded (URLs, IDs, etc.).
fn should_skip_attr_encoding(name: &str) -> bool {
    matches!(
        name,
        "class" | "id" | "src" | "href" | "action" | "style" | "type" | "name" | "value"
    ) || ID_REF_ATTRS.contains(&name)
}

/// IE conditional comment (`<!--[if ...]>...<![endif]-->`): removing the
/// wrapper can change downlevel rendering, so it must survive comment stripping.
fn is_conditional_comment(text: &str) -> bool {
    let t = text.trim_start();
    t.starts_with("[if") || t.contains("[endif]")
}

/// Apply every obfuscation transform.
pub fn transform(html: &str, symbols: &SymbolMap, config: &ObfuscationConfig) -> Result<String> {
    let output = RefCell::new(Vec::with_capacity(html.len()));
    let rng = RefCell::new(match config.seed {
        Some(s) => StdRng::seed_from_u64(s),
        None => StdRng::from_rng(&mut rand::rng()),
    });

    // One salt per document keeps attribute ordering stable (gzip-friendly).
    let attr_salt: u64 = rng.borrow_mut().random_range(0..=u64::MAX);

    // Per-document structural scheme (random attr/key/reverse) defeats fixed decoders.
    let structural_scheme = config
        .structural_obfuscation
        .then(|| structural::Scheme::new(&mut rng.borrow_mut()));

    // Honeypot decoys with a random marker, stripped on load by a removal script.
    let honeypots = config
        .inject_honeypots
        .then(|| honeypot::Honeypots::new(&mut rng.borrow_mut()));

    let remove_comments = config.remove_comments;
    let collapse_ws = config.collapse_whitespace;
    let encode_text = config.encode_text_entities;
    let encode_attrs = config.encode_attr_entities;
    let shuffle_attrs = config.shuffle_attributes;
    let randomize_case = config.randomize_tag_case;
    let split_words = config.split_words;
    let rename_classes = config.rename_classes;
    let rename_ids = config.rename_ids;
    let minify_css = config.minify_css;
    let unicode_escape = config.unicode_escape_selectors;
    // `Array` encoding needs the AST engine; without it, degrade to escapes.
    let js_encoding = match config.js_string_encoding {
        JsStringEncoding::Array if !config.js_ast => JsStringEncoding::Escapes,
        other => other,
    };
    let minify_js_opt = config.minify_js;
    let inject_honeypots = config.inject_honeypots;
    let honeypot_count = config.honeypot_count;
    let structural_obf = config.structural_obfuscation;
    let watermark_id = config.watermark;
    let emit_ai_opt_out = config.emit_ai_opt_out;
    let wants_ast = config.wants_ast();

    // Track preserved-whitespace context (Rc for sharing with end_tag_handlers)
    let preserved_depth = Rc::new(RefCell::new(0u32));

    // Depth inside <svg>/<math>, whose tag/attribute names are case-sensitive.
    let foreign_depth = Rc::new(RefCell::new(0u32));

    // Open-element stack (innermost last): the parent tag of a text node, for
    // structural obfuscation. Only elements with an end tag are pushed, so void
    // elements never unbalance it.
    let tag_stack: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    // Inside a <style> or <script> RAWTEXT element. Text there must not be
    // entity-encoded; browsers parse it as raw text.
    let in_raw_text = Rc::new(RefCell::new(false));

    // Whether the current <script> is real JS, not JSON/template/etc.
    let script_is_js = RefCell::new(true);

    // The watermark is embedded only once.
    let watermark_done = RefCell::new(false);

    // Accumulation buffers for style/script text (lol_html may split text chunks)
    let style_buf: RefCell<String> = RefCell::new(String::new());
    let script_buf: RefCell<String> = RefCell::new(String::new());
    // General text accumulator: collapsing per-chunk would double spaces at a
    // node split (lol-html#255), so buffer the whole node first.
    let text_buf: RefCell<String> = RefCell::new(String::new());

    let mut element_handlers: Vec<_> = Vec::new();
    let mut document_handlers: Vec<_> = Vec::new();

    if remove_comments {
        document_handlers.push(doc_comments!(|comment| {
            if !is_conditional_comment(&comment.text()) {
                comment.remove();
            }
            Ok(())
        }));
    }

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

    element_handlers.push(element!("script", |el| {
        *script_buf.borrow_mut() = String::new();
        *in_raw_text.borrow_mut() = true;

        let is_js = match el.get_attribute("type") {
            Some(t) => {
                let t = t.to_ascii_lowercase();
                t.is_empty() || t.contains("javascript") || t.contains("ecmascript")
            },
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

    element_handlers.push(element!("*", |el| {
        let tag_lower = el.tag_name().to_ascii_lowercase();

        // Maintain the open-element stack (only for elements that have an end
        // tag, keeping it balanced regardless of void/self-closing elements).
        // Gives the text handler its parent tag (content scoping + whitespace drop).
        if structural_obf || watermark_id.is_some() || split_words || collapse_ws {
            if let Some(handlers) = el.end_tag_handlers() {
                tag_stack.borrow_mut().push(tag_lower.clone());
                let stack = Rc::clone(&tag_stack);
                let handler: lol_html::EndTagHandler<'static> = Box::new(move |_end| {
                    stack.borrow_mut().pop();
                    Ok(())
                });
                handlers.push(handler);
            }
        }

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

        // Track foreign content; incrementing here also covers the root itself.
        if tag_lower == "svg" || tag_lower == "math" {
            *foreign_depth.borrow_mut() += 1;
            if let Some(handlers) = el.end_tag_handlers() {
                let depth = Rc::clone(&foreign_depth);
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

        if rename_classes {
            if let Some(class_attr) = el.get_attribute("class") {
                let new_classes: Vec<&str> = class_attr
                    .split_whitespace()
                    .map(|c| symbols.get_class(c).unwrap_or(c))
                    .collect();
                el.set_attribute("class", &new_classes.join(" "))?;
            }
        }

        if rename_ids {
            if let Some(id_attr) = el.get_attribute("id") {
                if let Some(new_id) = symbols.get_id(&id_attr) {
                    el.set_attribute("id", new_id)?;
                }
            }

            for &attr_name in ID_REF_ATTRS {
                if let Some(value) = el.get_attribute(attr_name) {
                    let new_value: Vec<&str> = value
                        .split_whitespace()
                        .map(|id| symbols.get_id(id).unwrap_or(id))
                        .collect();
                    el.set_attribute(attr_name, &new_value.join(" "))?;
                }
            }

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

        // Encode attribute values (skip functional attrs like IDs, URLs).
        // Skip in foreign content: rewriting lowercases names via lol_html,
        // breaking case-sensitive SVG names like `viewBox`.
        let in_foreign = *foreign_depth.borrow() > 0;
        if encode_attrs && !in_foreign {
            let mut rng = rng.borrow_mut();
            let attrs: Vec<(String, String)> = el.attributes().iter().map(|a| (a.name(), a.value())).collect();
            for (name, value) in &attrs {
                if should_skip_attr_encoding(name) {
                    continue;
                }
                let encoded = entities::encode_attr_value(value, &mut rng);
                el.set_attribute(name, &encoded)?;
            }
        }

        if shuffle_attrs && !in_foreign {
            let mut attrs: Vec<(String, String)> = el.attributes().iter().map(|a| (a.name(), a.value())).collect();
            tags::shuffle_attributes(&mut attrs, attr_salt);

            let attr_names: Vec<String> = el.attributes().iter().map(|a| a.name()).collect();
            for name in &attr_names {
                el.remove_attribute(name);
            }
            for (name, value) in &attrs {
                el.set_attribute(name, value)?;
            }
        }

        // HTML tag names only; foreign (SVG/MathML) names are case-sensitive.
        if randomize_case && !in_foreign {
            let mut rng = rng.borrow_mut();
            let new_tag = tags::randomize_tag_case(&tag_lower, &mut rng);
            el.set_tag_name(&new_tag)?;
        }

        Ok(())
    }));

    // Handles all text nodes except RAWTEXT context.
    element_handlers.push(text!("*", |text| {
        // <style>/<script> text is handled by dedicated handlers.
        if *in_raw_text.borrow() {
            return Ok(());
        }

        // Buffer the whole node before processing (see text_buf).
        text_buf.borrow_mut().push_str(text.as_str());
        if !text.last_in_text_node() {
            text.remove();
            return Ok(());
        }

        let content = std::mem::take(&mut *text_buf.borrow_mut());
        if content.is_empty() {
            return Ok(());
        }

        let is_preserved = *preserved_depth.borrow() > 0;

        // Drop whitespace-only text inside table/select containers: the parser
        // never renders it, so removing it is safe regardless of CSS.
        if collapse_ws && !is_preserved && content.trim().is_empty() {
            let in_container = tag_stack
                .borrow()
                .last()
                .map(|t| whitespace::is_whitespace_container(t))
                .unwrap_or(false);
            if in_container {
                text.replace("", ContentType::Html);
                return Ok(());
            }
        }

        let mut processed = content;

        if collapse_ws && !is_preserved {
            processed = whitespace::collapse_whitespace(&processed);
        }

        // Watermark/structural apply only to normal flow content (not <title>/metadata).
        let parent_is_safe = tag_stack
            .borrow()
            .last()
            .map(|t| structural::is_safe_tag(t))
            .unwrap_or(false);

        // Before the structural/entity passes so it travels with the text either way.
        if let Some(id) = watermark_id {
            if !is_preserved && parent_is_safe && !*watermark_done.borrow() && !processed.trim().is_empty() {
                processed = format!("{}{processed}", watermark::embed(id));
                *watermark_done.borrow_mut() = true;
            }
        }

        // Structural obfuscation: relocate non-blank text inside safe flow
        // elements into an encoded data-attribute, restored client-side.
        if let Some(scheme) = &structural_scheme {
            if !is_preserved && parent_is_safe && !processed.trim().is_empty() {
                text.replace(&scheme.encode_text_node(&processed), ContentType::Html);
                return Ok(());
            }
        }

        // Word-splitting (flow content only) interleaves the entity encoding so
        // the comment marker never lands inside an entity.
        if split_words && parent_is_safe && !is_preserved {
            processed = word_split::split(&processed, encode_text, &mut rng.borrow_mut());
        } else if encode_text {
            let mut rng = rng.borrow_mut();
            processed = entities::encode_entities(&processed, &mut rng);
        }

        text.replace(&processed, ContentType::Html);
        Ok(())
    }));

    // Inject machine-readable AI opt-out signals at the start of <head>.
    if emit_ai_opt_out {
        element_handlers.push(element!("head", |el| {
            el.prepend(
                "<meta name=\"robots\" content=\"noai, noimageai\">\
<meta name=\"tdm-reservation\" content=\"1\">",
                ContentType::Html,
            );
            Ok(())
        }));
    }

    // Inject honeypots and the structural-restore script at the end of <body>.
    if inject_honeypots || structural_obf {
        element_handlers.push(element!("body", |el| {
            if let Some(hp) = &honeypots {
                let decoys = {
                    let mut r = rng.borrow_mut();
                    hp.generate(honeypot_count, &mut r)
                };
                el.append(&decoys, ContentType::Html);
                el.append(&hp.removal_script(), ContentType::Html);
            }
            if let Some(scheme) = &structural_scheme {
                el.append(&scheme.restore_script(), ContentType::Html);
            }
            Ok(())
        }));
    }

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

    element_handlers.push(text!("script", |text| {
        script_buf.borrow_mut().push_str(text.as_str());

        if text.last_in_text_node() {
            let content = script_buf.borrow().clone();
            if !content.is_empty() {
                if *script_is_js.borrow() {
                    // JavaScript: AST engine when requested, else the token path.
                    // The AST path falls back to the token path on parse failure.
                    let mut rng_ref = rng.borrow_mut();
                    let token_path = |rng: &mut StdRng| {
                        js::transform_js(
                            &content,
                            symbols,
                            js_encoding,
                            minify_js_opt,
                            rename_classes,
                            rename_ids,
                            rng,
                        )
                    };
                    let transformed = if wants_ast {
                        js_ast::transform(&content, symbols, config, &mut rng_ref)
                            .unwrap_or_else(|| token_path(&mut rng_ref))
                    } else {
                        token_path(&mut rng_ref)
                    };
                    text.replace(&transformed, ContentType::Html);
                } else if rename_classes || rename_ids {
                    // Non-JS (JSON, etc.): only rename class/ID references
                    let transformed = js::replace_symbols_word_boundary(&content, symbols, rename_classes, rename_ids);
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
        let mut settings = Settings::new();
        for handler in element_handlers {
            settings = settings.append_element_content_handler(handler);
        }
        for handler in document_handlers {
            settings = settings.append_document_content_handler(handler);
        }
        let mut rewriter = HtmlRewriter::new(settings, |chunk: &[u8]| {
            output.borrow_mut().extend_from_slice(chunk);
        });

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
            js_string_encoding: JsStringEncoding::None,
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

    #[test]
    fn keeps_ie_conditional_comments() {
        let html = "<div><!-- drop me --><!--[if IE]><b>old</b><![endif]-->x</div>";
        let symbols = SymbolMap::new(Some(42));
        let result = transform(html, &symbols, &default_config()).unwrap();
        assert!(
            result.contains("[if IE]"),
            "conditional comment must survive removal: {result}"
        );
        assert!(!result.contains("drop me"), "normal comment must be removed: {result}");
    }

    #[test]
    fn preserves_svg_camelcase_tags_and_attrs() {
        let html = r#"<svg viewBox="0 0 10 10"><linearGradient gradientUnits="userSpaceOnUse"><stop></stop></linearGradient></svg>"#;
        let config = ObfuscationConfig {
            seed: Some(42),
            rename_classes: false,
            rename_ids: false,
            // entity/shuffle/case all stay on: none may corrupt foreign names.
            ..Default::default()
        };
        let symbols = SymbolMap::new(Some(42));
        let result = transform(html, &symbols, &config).unwrap();
        assert!(
            result.contains("linearGradient"),
            "SVG camelCase tag must survive: {result}"
        );
        assert!(result.contains("viewBox"), "SVG camelCase attr must survive: {result}");
        assert!(
            result.contains("gradientUnits"),
            "SVG camelCase attr must survive: {result}"
        );
    }

    #[test]
    fn split_text_node_no_double_space() {
        // A large text node forces lol_html to deliver multiple chunks;
        // collapsing each independently would emit doubled spaces (#255).
        let html = format!("<p>{}</p>", "word    ".repeat(5000));
        let config = ObfuscationConfig {
            seed: Some(42),
            encode_text_entities: false,
            randomize_tag_case: false,
            shuffle_attributes: false,
            rename_classes: false,
            rename_ids: false,
            ..Default::default()
        };
        let symbols = SymbolMap::new(Some(42));
        let result = transform(&html, &symbols, &config).unwrap();
        assert!(
            !result.contains("  "),
            "collapsed text must not contain doubled spaces across chunk splits"
        );
    }

    #[test]
    fn watermark_embeds_in_content_not_title() {
        let id = 0x1122_3344_5566_7788;
        let html = "<html><head><title>T</title></head><body><p>real content here</p></body></html>";
        let config = ObfuscationConfig {
            seed: Some(42),
            watermark: Some(id),
            encode_text_entities: false,
            randomize_tag_case: false,
            rename_classes: false,
            rename_ids: false,
            ..Default::default()
        };
        let symbols = SymbolMap::new(Some(42));
        let result = transform(html, &symbols, &config).unwrap();
        assert!(result.contains("<title>T</title>"), "title must stay clean: {result}");
        assert_eq!(watermark::decode(&result), Some(id), "id must be recoverable");
    }

    #[test]
    fn ai_opt_out_injects_meta_into_head() {
        let html = "<html><head><title>t</title></head><body><p>x</p></body></html>";
        let config = ObfuscationConfig {
            seed: Some(42),
            emit_ai_opt_out: true,
            randomize_tag_case: false,
            ..Default::default()
        };
        let symbols = SymbolMap::new(Some(42));
        let result = transform(html, &symbols, &config).unwrap();
        assert!(result.contains(r#"<meta name="robots" content="noai, noimageai">"#));
        assert!(result.contains(r#"<meta name="tdm-reservation" content="1">"#));
    }
}
