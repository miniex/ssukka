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

    let result = Obfuscator::builder()
        .seed(42)
        .build()
        .obfuscate(html)
        .unwrap();

    // Original class/ID names should not appear
    assert!(!result.contains(r#"class="container""#));
    assert!(!result.contains(r#"class="item""#));
    assert!(!result.contains(r#"id="header""#));
    assert!(!result.contains(r#"id="footer""#));

    // HTML comments should be removed (none in this test but verify structure)
    assert!(!result.contains("<!--"));

    // Output should still be valid-ish HTML
    assert!(result.contains("<"));
    assert!(result.contains("</"));
}

#[test]
fn deterministic_output_with_seed() {
    let html = r#"<div class="foo" id="bar">text</div>"#;

    let r1 = Obfuscator::builder()
        .seed(42)
        .build()
        .obfuscate(html)
        .unwrap();

    let r2 = Obfuscator::builder()
        .seed(42)
        .build()
        .obfuscate(html)
        .unwrap();

    assert_eq!(r1, r2);
}

#[test]
fn different_seeds_produce_different_output() {
    let html = r#"<div class="foo">text</div>"#;

    let r1 = Obfuscator::builder()
        .seed(1)
        .build()
        .obfuscate(html)
        .unwrap();

    let r2 = Obfuscator::builder()
        .seed(2)
        .build()
        .obfuscate(html)
        .unwrap();

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

    let result = Obfuscator::builder()
        .seed(42)
        .build()
        .obfuscate(html)
        .unwrap();

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
