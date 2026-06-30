use crate::error::{Result, SsukkaError};
use crate::symbol_map::SymbolMap;

/// Extract class and ID names from CSS source text.
///
/// Uses lightningcss to parse the CSS and walks all rules to find
/// class selectors (`.foo`) and ID selectors (`#bar`).
pub fn extract_selectors(css: &str, symbols: &mut SymbolMap, rename_classes: bool, rename_ids: bool) {
    let stylesheet =
        lightningcss::stylesheet::StyleSheet::parse(css, lightningcss::stylesheet::ParserOptions::default());

    let stylesheet = match stylesheet {
        Ok(s) => s,
        Err(_) => return,
    };

    for rule in stylesheet.rules.0.iter() {
        extract_from_rule(rule, symbols, rename_classes, rename_ids);
    }
}

fn extract_from_rule(
    rule: &lightningcss::rules::CssRule,
    symbols: &mut SymbolMap,
    rename_classes: bool,
    rename_ids: bool,
) {
    use lightningcss::rules::CssRule;

    match rule {
        CssRule::Style(style_rule) => {
            for selector in style_rule.selectors.0.iter() {
                for component in selector.iter_raw_match_order() {
                    match component {
                        lightningcss::selector::Component::Class(name) if rename_classes => {
                            symbols.register_class(&name.0);
                        },
                        lightningcss::selector::Component::ID(name) if rename_ids => {
                            symbols.register_id(&name.0);
                        },
                        _ => {},
                    }
                }
            }
            for nested in style_rule.rules.0.iter() {
                extract_from_rule(nested, symbols, rename_classes, rename_ids);
            }
        },
        CssRule::Media(media) => {
            for r in media.rules.0.iter() {
                extract_from_rule(r, symbols, rename_classes, rename_ids);
            }
        },
        CssRule::Supports(supports) => {
            for r in supports.rules.0.iter() {
                extract_from_rule(r, symbols, rename_classes, rename_ids);
            }
        },
        // Keyframe names (grouped with class renaming). Only names *defined* here
        // are registered, so a reference to an external keyframe stays intact.
        CssRule::Keyframes(kf) if rename_classes => {
            use lightningcss::rules::keyframes::KeyframesName;
            let name = match &kf.name {
                KeyframesName::Ident(id) => &id.0,
                KeyframesName::Custom(s) => s,
            };
            symbols.register_keyframe(name);
        },
        _ => {},
    }
}

/// lightningcss visitor that renames `@keyframes` names and their
/// `animation` / `animation-name` references (all `CustomIdent`s) via the map.
struct KeyframeRenamer<'a> {
    symbols: &'a SymbolMap,
}

impl<'i> lightningcss::visitor::Visitor<'i> for KeyframeRenamer<'_> {
    type Error = std::convert::Infallible;

    fn visit_types(&self) -> lightningcss::visitor::VisitTypes {
        lightningcss::visitor::VisitTypes::CUSTOM_IDENTS
    }

    fn visit_custom_ident(
        &mut self,
        ident: &mut lightningcss::values::ident::CustomIdent<'_>,
    ) -> std::result::Result<(), Self::Error> {
        if let Some(new) = self.symbols.get_keyframe(ident.0.as_ref()) {
            ident.0 = new.to_string().into();
        }
        Ok(())
    }
}

/// Transform CSS source: rename classes/IDs, minify, and unicode-escape selectors.
pub fn transform_css(
    css: &str,
    symbols: &SymbolMap,
    minify: bool,
    unicode_escape: bool,
    rename_classes: bool,
    rename_ids: bool,
) -> Result<String> {
    let mut result = css.to_owned();

    // Rename only inside selector preludes, so a class/ID can't corrupt a
    // declaration value sharing its text (e.g. `color:#abc`, `content:".x"`).
    if rename_classes || rename_ids {
        result = rename_selectors(&result, symbols, rename_classes, rename_ids);
    }

    // Rename keyframe/animation names via a lightningcss visitor (handles the
    // shorthand). Shares the parse/print with minification when both are on.
    let rename_kf = rename_classes && !symbols.keyframes().is_empty();
    if minify || rename_kf {
        use lightningcss::visitor::Visit;
        // Parse from an owned copy so the borrow is scoped.
        let to_parse = result.clone();
        let mut stylesheet =
            lightningcss::stylesheet::StyleSheet::parse(&to_parse, lightningcss::stylesheet::ParserOptions::default())
                .map_err(|e| SsukkaError::Css(e.to_string()))?;

        if rename_kf {
            let _ = stylesheet.visit(&mut KeyframeRenamer { symbols });
        }

        let print_options = lightningcss::printer::PrinterOptions {
            minify,
            ..Default::default()
        };
        let output = stylesheet
            .to_css(print_options)
            .map_err(|e| SsukkaError::Css(e.to_string()))?;
        result = output.code;
    }

    if unicode_escape {
        result = unicode_escape_selectors(&result);
    }

    Ok(result)
}

/// Rename `.class` / `#id` selectors via the [`SymbolMap`], touching only
/// selector preludes (see [`map_selector_preludes`]).
fn rename_selectors(css: &str, symbols: &SymbolMap, rename_classes: bool, rename_ids: bool) -> String {
    map_selector_preludes(css, |prelude| {
        rewrite_prelude_names(
            prelude,
            |name| {
                if rename_classes {
                    symbols.get_class(name).map(str::to_owned)
                } else {
                    None
                }
            },
            |name| {
                if rename_ids {
                    symbols.get_id(name).map(str::to_owned)
                } else {
                    None
                }
            },
        )
    })
}

/// Apply unicode escape sequences to class and ID selectors in CSS.
///
/// Converts `.foo` -> `.\66 \6f \6f ` and `#bar` -> `#\62 \61 \72 `. Only
/// selector preludes are escaped, so hex colors / value tokens are untouched.
fn unicode_escape_selectors(css: &str) -> String {
    map_selector_preludes(css, |prelude| {
        rewrite_prelude_names(prelude, |name| Some(escape_name(name)), |name| Some(escape_name(name)))
    })
}

/// Escape every char of a selector name as `\<hex> ` (CSS unicode escape).
fn escape_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len() * 4);
    for ch in name.chars() {
        out.push('\\');
        out.push_str(&format!("{:x}", ch as u32));
        out.push(' ');
    }
    out
}

/// Run `f` over each selector-prelude region of `css` (the run of text that
/// ends with `{` at brace/paren depth 0), copying declaration blocks, strings,
/// comments, and at-rule statements through verbatim.
fn map_selector_preludes(css: &str, mut f: impl FnMut(&str) -> String) -> String {
    let chars: Vec<char> = css.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(css.len());
    let mut seg = String::new();
    let mut paren: u32 = 0;
    let mut i = 0;

    while i < n {
        let c = chars[i];

        // Comments and strings pass through into the current segment verbatim.
        if c == '/' && i + 1 < n && chars[i + 1] == '*' {
            seg.push('/');
            seg.push('*');
            i += 2;
            while i < n {
                seg.push(chars[i]);
                if chars[i] == '*' && i + 1 < n && chars[i + 1] == '/' {
                    seg.push('/');
                    i += 2;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if c == '"' || c == '\'' {
            seg.push(c);
            i += 1;
            while i < n {
                let d = chars[i];
                seg.push(d);
                i += 1;
                if d == '\\' && i < n {
                    seg.push(chars[i]);
                    i += 1;
                } else if d == c {
                    break;
                }
            }
            continue;
        }

        // Track paren depth so `;` / `{` inside `url(...)`, `:not(...)`, or a
        // media query don't prematurely terminate a segment.
        match c {
            '(' => {
                paren += 1;
                seg.push(c);
                i += 1;
            },
            ')' => {
                paren = paren.saturating_sub(1);
                seg.push(c);
                i += 1;
            },
            '{' if paren == 0 => {
                out.push_str(&f(&seg));
                out.push('{');
                seg.clear();
                i += 1;
            },
            '}' | ';' if paren == 0 => {
                out.push_str(&seg);
                out.push(c);
                seg.clear();
                i += 1;
            },
            _ => {
                seg.push(c);
                i += 1;
            },
        }
    }
    out.push_str(&seg);
    out
}

/// Rewrite `.class` / `#id` names within a selector prelude. Strings and
/// comments are skipped (e.g. attribute-selector values). The callbacks receive
/// the bare name and return its replacement, or `None` to leave it unchanged.
fn rewrite_prelude_names(
    prelude: &str,
    mut on_class: impl FnMut(&str) -> Option<String>,
    mut on_id: impl FnMut(&str) -> Option<String>,
) -> String {
    let chars: Vec<char> = prelude.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(prelude.len());
    let mut i = 0;

    while i < n {
        let c = chars[i];
        if c == '/' && i + 1 < n && chars[i + 1] == '*' {
            out.push('/');
            out.push('*');
            i += 2;
            while i < n {
                out.push(chars[i]);
                if chars[i] == '*' && i + 1 < n && chars[i + 1] == '/' {
                    out.push('/');
                    i += 2;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if c == '"' || c == '\'' {
            out.push(c);
            i += 1;
            while i < n {
                let d = chars[i];
                out.push(d);
                i += 1;
                if d == '\\' && i < n {
                    out.push(chars[i]);
                    i += 1;
                } else if d == c {
                    break;
                }
            }
            continue;
        }

        // `.`/`#` + an ident start (CSS idents never start with a digit).
        if (c == '.' || c == '#')
            && i + 1 < n
            && (chars[i + 1].is_ascii_alphabetic() || chars[i + 1] == '_' || chars[i + 1] == '-')
        {
            let start = i + 1;
            let mut j = start;
            while j < n && (chars[j].is_ascii_alphanumeric() || chars[j] == '_' || chars[j] == '-') {
                j += 1;
            }
            let name: String = chars[start..j].iter().collect();
            let repl = if c == '.' { on_class(&name) } else { on_id(&name) };
            out.push(c);
            out.push_str(repl.as_deref().unwrap_or(&name));
            i = j;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_class_selectors() {
        let css = ".foo { color: red; } .bar .baz { display: flex; }";
        let mut symbols = SymbolMap::new(Some(42));
        extract_selectors(css, &mut symbols, true, true);
        assert!(symbols.get_class("foo").is_some());
        assert!(symbols.get_class("bar").is_some());
        assert!(symbols.get_class("baz").is_some());
    }

    #[test]
    fn extract_id_selectors() {
        let css = "#main { width: 100%; } #sidebar { float: left; }";
        let mut symbols = SymbolMap::new(Some(42));
        extract_selectors(css, &mut symbols, true, true);
        assert!(symbols.get_id("main").is_some());
        assert!(symbols.get_id("sidebar").is_some());
    }

    #[test]
    fn unicode_escape() {
        let result = unicode_escape_selectors(".foo{color:red}");
        assert!(result.contains('\\'));
        assert!(!result.contains(".foo"));
    }

    #[test]
    fn minify_css() {
        let css = ".foo  {  color:  red;  }";
        let symbols = SymbolMap::new(Some(42));
        let result = transform_css(css, &symbols, true, false, false, false).unwrap();
        assert!(!result.contains("  "));
    }

    #[test]
    fn id_rename_skips_hex_color_values() {
        // An ID named like a hex color must not corrupt `#abc` color values.
        let mut symbols = SymbolMap::new(Some(1));
        symbols.register_id("abc");
        let obf = symbols.get_id("abc").unwrap().to_owned();
        let css = "#abc{color:#abcdef;background:#abc}";
        let out = transform_css(css, &symbols, false, false, false, true).unwrap();
        assert!(out.contains(&format!("#{obf}{{")), "selector should be renamed: {out}");
        assert!(out.contains("#abcdef"), "long hex color preserved: {out}");
        assert!(out.contains("#abc}"), "short hex color value preserved: {out}");
    }

    #[test]
    fn class_rename_skips_value_strings() {
        // `.foo` inside a `content` string must survive; the selector is renamed.
        let mut symbols = SymbolMap::new(Some(1));
        symbols.register_class("foo");
        let obf = symbols.get_class("foo").unwrap().to_owned();
        let css = r#".foo{content:".foo"}"#;
        let out = transform_css(css, &symbols, false, false, true, false).unwrap();
        assert!(out.contains(&format!(".{obf}{{")), "selector renamed: {out}");
        assert!(out.contains(r#"content:".foo""#), "string value preserved: {out}");
    }

    #[test]
    fn unicode_escape_skips_value_tokens() {
        // Only the selector is escaped; the `#fff` color value is left alone.
        let out = unicode_escape_selectors(".foo{color:#fff}");
        assert!(!out.contains(".foo"), "selector escaped: {out}");
        assert!(out.contains("#fff"), "hex color value untouched: {out}");
    }

    #[test]
    fn renames_keyframes_and_animation_references() {
        let css = "@keyframes spin{from{opacity:0}to{opacity:1}}.box{animation:spin 2s linear;animation-name:spin}";
        let mut symbols = SymbolMap::new(Some(1));
        extract_selectors(css, &mut symbols, true, true);
        let kf = symbols.get_keyframe("spin").expect("keyframe registered").to_owned();
        let out = transform_css(css, &symbols, true, false, true, false).unwrap();
        assert!(
            out.contains(&format!("@keyframes {kf}")),
            "keyframes def renamed: {out}"
        );
        assert!(out.contains(&kf), "animation reference renamed: {out}");
        assert!(!out.contains("spin"), "no original keyframe name remains: {out}");
    }

    #[test]
    fn keyframe_rename_skips_undefined_animation_names() {
        // An animation referencing a keyframe not defined here must stay intact.
        let css = ".box{animation-name:external}";
        let symbols = SymbolMap::new(Some(1));
        let out = transform_css(css, &symbols, true, false, true, false).unwrap();
        assert!(out.contains("external"), "undefined keyframe ref preserved: {out}");
    }

    #[test]
    fn rename_handles_nested_and_media_rules() {
        let mut symbols = SymbolMap::new(Some(1));
        symbols.register_class("a");
        symbols.register_class("b");
        let a = symbols.get_class("a").unwrap().to_owned();
        let b = symbols.get_class("b").unwrap().to_owned();
        let css = "@media (min-width:600px){.a{color:#aaa;.b{color:red}}}";
        let out = transform_css(css, &symbols, false, false, true, false).unwrap();
        assert!(out.contains(&format!(".{a}")), "outer class renamed: {out}");
        assert!(out.contains(&format!(".{b}")), "nested class renamed: {out}");
        assert!(out.contains("#aaa"), "color value preserved: {out}");
    }
}
