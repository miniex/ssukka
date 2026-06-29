use ssukka::Obfuscator;

#[test]
fn full_obfuscation_pipeline() {
    let html = r##"<!DOCTYPE html>
<html>
<head>
    <style>
        .container { display: flex; }
        #header { color: blue; }
        .item { padding: 10px; }
    </style>
</head>
<body>
    <div id="header" class="container">
        <span class="item">Hello World</span>
        <a href="#footer">Go to footer</a>
    </div>
    <div id="footer" class="container">
        Footer content
    </div>
    <script>
        document.getElementById("header");
        document.querySelector(".item");
        el.classList.add("active");
    </script>
</body>
</html>"##;

    let result = Obfuscator::builder().seed(42).build().obfuscate(html).unwrap();

    // Original class/ID names should not appear
    assert!(!result.contains(r#"class="container""#));
    assert!(!result.contains(r#"class="item""#));
    assert!(!result.contains(r#"id="header""#));
    assert!(!result.contains(r#"id="footer""#));

    // HTML comments are removed (none here, just check structure).
    assert!(!result.contains("<!--"));

    // Output should still be valid-ish HTML
    assert!(result.contains("<"));
    assert!(result.contains("</"));
}

#[test]
fn deterministic_output_with_seed() {
    let html = r#"<div class="foo" id="bar">text</div>"#;

    let r1 = Obfuscator::builder().seed(42).build().obfuscate(html).unwrap();

    let r2 = Obfuscator::builder().seed(42).build().obfuscate(html).unwrap();

    assert_eq!(r1, r2);
}

#[test]
fn different_seeds_produce_different_output() {
    let html = r#"<div class="foo">text</div>"#;

    let r1 = Obfuscator::builder().seed(1).build().obfuscate(html).unwrap();

    let r2 = Obfuscator::builder().seed(2).build().obfuscate(html).unwrap();

    assert_ne!(r1, r2);
}

#[test]
fn no_rename_preserves_class_names() {
    let html = r#"<div class="foo" id="bar">text</div>"#;

    let result = Obfuscator::builder()
        .seed(42)
        .rename_classes(false)
        .rename_ids(false)
        .build()
        .obfuscate(html)
        .unwrap();

    // class and id names should remain since renaming is disabled
    assert!(result.contains("foo"));
    assert!(result.contains("bar"));
}

#[test]
fn comment_removal() {
    let html = "<!-- secret --><div>visible</div><!-- hidden -->";

    let result = Obfuscator::builder().seed(42).build().obfuscate(html).unwrap();

    assert!(!result.contains("secret"));
    assert!(!result.contains("hidden"));
    assert!(!result.contains("<!--"));
}

#[test]
fn whitespace_collapse() {
    let html = "<div>  lots   of    spaces  </div>";

    let result = Obfuscator::builder()
        .seed(42)
        .encode_text_entities(false)
        .randomize_tag_case(false)
        .shuffle_attributes(false)
        .build()
        .obfuscate(html)
        .unwrap();

    assert!(!result.contains("   "));
}

#[test]
fn pre_whitespace_preserved() {
    let html = "<pre>  code  \n  here  </pre>";

    let result = Obfuscator::builder()
        .seed(42)
        .encode_text_entities(false)
        .randomize_tag_case(false)
        .shuffle_attributes(false)
        .build()
        .obfuscate(html)
        .unwrap();

    assert!(result.contains("  code  \n  here  "));
}

#[test]
fn entity_encoding() {
    let html = "<div>Hello World</div>";

    let result = Obfuscator::builder()
        .seed(42)
        .randomize_tag_case(false)
        .shuffle_attributes(false)
        .build()
        .obfuscate(html)
        .unwrap();

    // Text should be entity-encoded (contain &#xx; or &#xNN; patterns)
    assert!(result.contains("&#") || result.contains("&amp;") || result.contains("&lt;"));
}

#[test]
fn css_minification() {
    let html = "<style>.foo  {  color:  red;  }</style>";

    let result = Obfuscator::builder()
        .seed(42)
        .rename_classes(false)
        .unicode_escape_selectors(false)
        .randomize_tag_case(false)
        .shuffle_attributes(false)
        .build()
        .obfuscate(html)
        .unwrap();

    // CSS should be minified
    assert!(!result.contains("  color:  red;  "));
}

#[test]
fn js_string_encoding() {
    let html = r#"<script>var x = "hello";</script>"#;

    let result = Obfuscator::builder()
        .seed(42)
        .randomize_tag_case(false)
        .shuffle_attributes(false)
        .build()
        .obfuscate(html)
        .unwrap();

    // "hello" should be encoded
    assert!(!result.contains(r#""hello""#));
}

#[test]
fn consistent_class_rename_across_html_and_css() {
    let html = r#"<style>.myclass { color: red; }</style><div class="myclass">content</div>"#;

    let result = Obfuscator::builder()
        .seed(42)
        .encode_text_entities(false)
        .randomize_tag_case(false)
        .shuffle_attributes(false)
        .unicode_escape_selectors(false)
        .minify_css(false)
        .build()
        .obfuscate(html)
        .unwrap();

    // "myclass" should not appear in either CSS or HTML
    assert!(
        !result.contains("myclass"),
        "Original class name should be renamed everywhere. Got: {result}"
    );
}

#[test]
fn consistent_id_rename_across_html_css_js() {
    let html = r##"<style>#myid { color: red; }</style>
<div id="myid">content</div>
<a href="#myid">link</a>
<label for="myid">label</label>
<script>document.getElementById("myid");</script>"##;

    let result = Obfuscator::builder()
        .seed(42)
        .encode_text_entities(false)
        .encode_js_strings(false)
        .randomize_tag_case(false)
        .shuffle_attributes(false)
        .unicode_escape_selectors(false)
        .minify_css(false)
        .minify_js(false)
        .build()
        .obfuscate(html)
        .unwrap();

    assert!(
        !result.contains("myid"),
        "Original ID should be renamed everywhere. Got: {result}"
    );
}

#[test]
fn simple_api() {
    let html = "<div>Hello</div>";
    let result = ssukka::obfuscate(html).unwrap();
    // Should produce some output
    assert!(!result.is_empty());
}

// Advanced (opt-in) features

#[test]
fn honeypots_are_injected_and_hidden() {
    let html = "<html><body><p>real</p></body></html>";
    let result = Obfuscator::builder()
        .seed(42)
        .inject_honeypots(true)
        .honeypot_count(5)
        .build()
        .obfuscate(html)
        .unwrap();
    // Decoys are present but inert (hidden + aria-hidden + non-focusable).
    assert_eq!(result.matches("aria-hidden=\"true\"").count(), 5);
    assert!(result.contains("display:none"));
    assert!(result.contains("tabindex=\"-1\""));
    // A removal script strips them for JS clients, leaving no signature.
    assert!(result.contains("e.remove()"));
}

#[test]
fn structural_obfuscation_hides_text_and_injects_restore() {
    let html = "<html><body><p>Secret paragraph text</p></body></html>";
    let result = Obfuscator::builder()
        .seed(42)
        .structural_obfuscation(true)
        .build()
        .obfuscate(html)
        .unwrap();
    // The visible text is gone from static markup; a restore hook remains.
    assert!(!result.contains("Secret paragraph text"));
    assert!(
        result.contains("<span data-"),
        "payload span with a data- attr: {result}"
    );
    assert!(result.contains("TextDecoder"));
    // The old fixed scheme must no longer be emitted verbatim.
    assert!(!result.contains("data-ssk"));
}

#[test]
fn polymorphic_output_varies_without_seed() {
    let html = r#"<div class="a"><p>hello world</p></div>"#;
    let r1 = Obfuscator::builder()
        .polymorphic(true)
        .inject_honeypots(true)
        .build()
        .obfuscate(html)
        .unwrap();
    let r2 = Obfuscator::builder()
        .polymorphic(true)
        .inject_honeypots(true)
        .build()
        .obfuscate(html)
        .unwrap();
    assert_ne!(r1, r2);
}

#[test]
fn ast_mangle_hides_local_identifiers() {
    let html = r#"<script>function compute(alpha, beta) { var gamma = alpha + beta; return gamma; }</script>"#;
    let result = Obfuscator::builder()
        .seed(42)
        .js_ast(true)
        .mangle_identifiers(true)
        .build()
        .obfuscate(html)
        .unwrap();
    // Local binding names are mangled away.
    assert!(!result.contains("gamma"));
    assert!(!result.contains("alpha"));
}

#[test]
fn ast_string_array_hides_literals() {
    let html = r#"<script>var a = "first secret"; var b = "second secret"; use(a, b);</script>"#;
    let result = Obfuscator::builder()
        .seed(42)
        .js_ast(true)
        .js_string_encoding(ssukka::config::JsStringEncoding::Array)
        .build()
        .obfuscate(html)
        .unwrap();
    assert!(!result.contains("first secret"));
    assert!(!result.contains("second secret"));
    assert!(result.contains("atob"));
}

#[test]
fn ast_falls_back_gracefully_on_unparsable_js() {
    // Deliberately broken JS: the AST engine must not panic or drop the script,
    // it falls back to the token path.
    let html = r#"<script>function ( { this is not valid js ===</script>"#;
    let result = Obfuscator::builder()
        .seed(42)
        .js_ast(true)
        .mangle_identifiers(true)
        .build()
        .obfuscate(html);
    assert!(result.is_ok());
}

#[test]
fn ast_class_rename_still_applies_in_js() {
    let html =
        r#"<style>.box{color:red}</style><div class="box"></div><script>document.querySelector(".box");</script>"#;
    let result = Obfuscator::builder()
        .seed(42)
        .js_ast(true)
        .mangle_identifiers(true)
        .build()
        .obfuscate(html)
        .unwrap();
    // Class renaming is consistent even on the AST path.
    assert!(!result.contains("\"box\""));
    assert!(!result.contains(".box"));
}
