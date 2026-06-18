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
        _ => {},
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

    if rename_classes {
        let mut class_pairs: Vec<_> = symbols.classes().iter().collect();
        class_pairs.sort_by_key(|b| std::cmp::Reverse(b.0.len()));
        for (original, obfuscated) in &class_pairs {
            result = result.replace(&format!(".{original}"), &format!(".{obfuscated}"));
        }
    }

    if rename_ids {
        let mut id_pairs: Vec<_> = symbols.ids().iter().collect();
        id_pairs.sort_by_key(|b| std::cmp::Reverse(b.0.len()));
        for (original, obfuscated) in &id_pairs {
            result = result.replace(&format!("#{original}"), &format!("#{obfuscated}"));
        }
    }

    if minify {
        // Parse from an owned copy so the borrow is scoped
        let to_parse = result.clone();
        let stylesheet =
            lightningcss::stylesheet::StyleSheet::parse(&to_parse, lightningcss::stylesheet::ParserOptions::default())
                .map_err(|e| SsukkaError::Css(e.to_string()))?;

        let print_options = lightningcss::printer::PrinterOptions {
            minify: true,
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

/// Apply unicode escape sequences to class and ID selectors in CSS.
///
/// Converts `.foo` -> `.\66\6f\6f` and `#bar` -> `#\62\61\72`
fn unicode_escape_selectors(css: &str) -> String {
    let mut out = String::with_capacity(css.len() * 2);
    let chars: Vec<char> = css.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if (chars[i] == '.' || chars[i] == '#') && i + 1 < len && is_selector_start(chars[i + 1]) {
            out.push(chars[i]);
            i += 1;
            while i < len && is_selector_char(chars[i]) {
                let code = chars[i] as u32;
                out.push('\\');
                out.push_str(&format!("{code:x}"));
                out.push(' ');
                i += 1;
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }

    out
}

fn is_selector_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_' || ch == '-' || ch == '\\'
}

fn is_selector_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
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
}
