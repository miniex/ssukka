//! OBsmith-style correctness harness: run representative snippets through the full
//! JS pipeline, execute the original and obfuscated forms under Node, and assert
//! identical output - catching semantic deviations, not just parse failures.
//! Skipped when `node` is unavailable.

use ssukka_core::config::JsStringEncoding;
use ssukka_core::Obfuscator;
use std::io::Write;
use std::process::{Command, Stdio};

/// DOM-free snippets that print to stdout, exercising values, control flow,
/// objects, recursion, numbers, and exceptions.
const SNIPPETS: &[&str] = &[
    "console.log([1,2,3,4].map(function(x){return x*x;}).reduce(function(a,b){return a+b;},0));",
    "var o={count:0,inc(){this.count++;return this.count;}};console.log(o.inc()+o.inc());",
    "function fib(n){return n<2?n:fib(n-1)+fib(n-2);}console.log(fib(12));",
    "var s='abcdef'.split('').reverse().join('');console.log(s,s.length,255,0xff);",
    "var m={alpha:1,beta:2};var t=0;for(var k in m){t+=m[k];}console.log(t,JSON.stringify(m));",
    "try{null.x;}catch(e){console.log('caught',e instanceof TypeError);}",
];

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run `js` under Node (program on stdin) and return stdout; panics on a non-zero exit.
fn run_node(js: &str) -> String {
    let mut child = Command::new("node")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn node");
    child.stdin.take().unwrap().write_all(js.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "node failed:\n{}\n--- program ---\n{js}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The `<script>` body of `html` (case-insensitive tags; one script expected).
fn extract_script(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let open = lower.find("<script").expect("script open");
    let gt = lower[open..].find('>').expect("script >") + open + 1;
    let close = lower[gt..].find("</script").expect("script close") + gt;
    html[gt..close].to_string()
}

/// Obfuscate `snippet` with every JS-level transform and return the result JS.
fn obfuscate_js(snippet: &str) -> String {
    let html = Obfuscator::builder()
        .seed(1)
        .js_ast(true)
        .mangle_identifiers(true)
        .js_string_encoding(JsStringEncoding::Array)
        .mba(true)
        .opaque_predicates(true)
        .dead_code_injection(true)
        .control_flow_flattening(true)
        .property_keys(true)
        .build()
        .obfuscate(&format!("<script>{snippet}</script>"))
        .expect("obfuscate");
    extract_script(&html)
}

#[test]
fn obfuscated_snippets_behave_identically_under_node() {
    if !node_available() {
        eprintln!("node unavailable; skipping OBsmith semantics test");
        return;
    }
    for snip in SNIPPETS {
        let obf = obfuscate_js(snip);
        let expected = run_node(snip);
        let actual = run_node(&obf);
        assert_eq!(
            expected, actual,
            "semantic deviation for: {snip}\n--- obfuscated ---\n{obf}"
        );
    }
}
