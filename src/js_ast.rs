//! AST-based JS obfuscation via [oxc](https://github.com/oxc-project/oxc):
//! local identifier mangling, minification, string arrays, dead code, and
//! control-flow flattening.
//!
//! Every rewriting step re-parses its output and is dropped if it would emit
//! invalid JS; a parse failure on input falls back to [`crate::js`]. Transforms
//! are semantics-preserving by construction.

use crate::config::ObfuscationConfig;
use crate::symbol_map::SymbolMap;
use oxc::allocator::Allocator;
use oxc::codegen::{Codegen, CodegenOptions};
use oxc::mangler::{MangleOptions, Mangler};
use oxc::parser::Parser;
use oxc::span::SourceType;
use rand::rngs::StdRng;
use rand::RngExt;

/// Run the AST pipeline. Returns `None` if the input cannot be parsed (the
/// caller should then fall back to the token-based path).
pub fn transform(js: &str, symbols: &SymbolMap, config: &ObfuscationConfig, rng: &mut StdRng) -> Option<String> {
    // Class/ID references inside string literals (reuse the token path).
    let mut code = if config.rename_classes || config.rename_ids {
        crate::js::replace_symbol_references(js, symbols, config.rename_classes, config.rename_ids)
    } else {
        js.to_owned()
    };

    // String arrays before codegen, which may turn literals into templates.
    if config.js_string_encoding == crate::config::JsStringEncoding::Array {
        if let Some(next) = string_array_pass(&code, rng) {
            code = next;
        }
    }

    // mangle + minify; None here means unparsable input -> caller falls back.
    code = mangle_and_print(&code, config)?;

    if config.dead_code_injection {
        if let Some(next) = dead_code_pass(&code, config.dead_code_threshold, rng) {
            code = next;
        }
    }
    if config.control_flow_flattening {
        if let Some(next) = cff_pass(&code, rng) {
            code = next;
        }
    }

    Some(code)
}

/// Parse, optionally mangle local bindings, and print (optionally minified).
///
/// Tries module then script source types so both modern and classic/sloppy
/// inline scripts are accepted. Returns `None` if neither parses cleanly.
fn mangle_and_print(js: &str, config: &ObfuscationConfig) -> Option<String> {
    for source_type in [SourceType::mjs(), SourceType::cjs()] {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, js, source_type).parse();
        if ret.panicked || !ret.diagnostics.is_empty() {
            continue;
        }
        let program = ret.program;

        let mut codegen = Codegen::new().with_options(CodegenOptions {
            minify: config.minify_js,
            ..CodegenOptions::default()
        });

        if config.mangle_identifiers {
            // top_level: false - never rename globals/top-level functions, which
            // may be referenced from other inline scripts or HTML event handlers.
            let mangled = Mangler::new()
                .with_options(MangleOptions {
                    top_level: Some(false),
                    debug: false,
                    ..MangleOptions::default()
                })
                .build(&program);
            codegen = codegen
                .with_scoping(Some(mangled.scoping))
                .with_private_member_mappings(Some(mangled.class_private_mappings));
        }

        return Some(codegen.build(&program).code);
    }
    None
}

/// Validate that `js` parses cleanly under either source type.
fn parses_ok(js: &str) -> bool {
    for source_type in [SourceType::mjs(), SourceType::cjs()] {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, js, source_type).parse();
        if !ret.panicked && ret.diagnostics.is_empty() {
            return true;
        }
    }
    false
}

/// Standard base64 (RFC 4648).
fn base64(input: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for c in input.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(A[((n >> 18) & 63) as usize] as char);
        out.push(A[((n >> 12) & 63) as usize] as char);
        out.push(if c.len() > 1 {
            A[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 { A[(n & 63) as usize] as char } else { '=' });
    }
    out
}

/// Random lowercase identifier suffix.
fn rand_id(rng: &mut StdRng, len: usize) -> String {
    const CH: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
    (0..len).map(|_| CH[rng.random_range(0..CH.len())] as char).collect()
}

/// Hoist string-literal *expressions* into a base64 array decoded at runtime.
///
/// Only `Expression::StringLiteral` nodes are rewritten, so property keys,
/// directives (`"use strict"`), and import/export sources - which are not
/// expression nodes - are left untouched. Returns `None` if there is nothing to
/// do or if the rewrite would not re-parse.
fn string_array_pass(js: &str, rng: &mut StdRng) -> Option<String> {
    let spans = collect_string_expr_spans(js)?;
    if spans.len() < 2 {
        return None;
    }

    let arr = format!("_{}", rand_id(rng, 6));
    let dec = format!("_{}", rand_id(rng, 6));

    // Splice from the end so earlier byte offsets stay valid.
    let mut out = js.to_owned();
    let mut values: Vec<(usize, String)> = Vec::with_capacity(spans.len());
    for (idx, (start, end, raw)) in spans.iter().enumerate().rev() {
        values.push((idx, raw.clone()));
        out.replace_range(*start..*end, &format!("{dec}({idx})"));
    }
    values.reverse();

    let mut prelude = format!("const {arr}=[");
    for (i, (_, raw)) in values.iter().enumerate() {
        if i > 0 {
            prelude.push(',');
        }
        prelude.push('"');
        prelude.push_str(&base64(raw.as_bytes()));
        prelude.push('"');
    }
    prelude.push_str(&format!(
        "];function {dec}(i){{var s=atob({arr}[i]),a=new Uint8Array(s.length);\
for(var j=0;j<s.length;j++)a[j]=s.charCodeAt(j);return new TextDecoder().decode(a);}}"
    ));

    let result = format!("{prelude}{out}");
    parses_ok(&result).then_some(result)
}

/// Collect (start, end, decoded-value) for every top-level-safe string literal
/// expression, using a span-recording visitor.
fn collect_string_expr_spans(js: &str) -> Option<Vec<(usize, usize, String)>> {
    use oxc::ast::ast::{Expression, PropertyKey};
    use oxc::ast_visit::{walk, Visit};

    struct Collector {
        spans: Vec<(usize, usize, String)>,
    }
    impl<'a> Visit<'a> for Collector {
        fn visit_expression(&mut self, expr: &Expression<'a>) {
            if let Expression::StringLiteral(lit) = expr {
                let s = lit.span;
                self.spans
                    .push((s.start as usize, s.end as usize, lit.value.to_string()));
            }
            walk::walk_expression(self, expr);
        }

        // A non-computed string property key (`{ "k": v }`) reaches us through
        // `visit_expression`, but replacing it with a call would be invalid.
        // Skip property keys entirely (their values are still visited).
        fn visit_property_key(&mut self, _key: &PropertyKey<'a>) {}
    }

    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, js, SourceType::cjs()).parse();
    if ret.panicked || !ret.diagnostics.is_empty() {
        return None;
    }
    let mut c = Collector { spans: Vec::new() };
    c.visit_program(&ret.program);
    Some(c.spans)
}

/// Prepend an always-false, block-scoped junk block. The predicate (`0xNNNN <
/// 0`) is never true and the body is `let`-scoped, so nothing leaks or runs.
fn dead_code_pass(js: &str, threshold: f32, rng: &mut StdRng) -> Option<String> {
    if threshold <= 0.0 {
        return None;
    }
    let count = ((threshold * 4.0).ceil() as usize).clamp(1, 6);
    let mut junk = String::new();
    for _ in 0..count {
        let pred = rng.random_range(0x1000u32..=0xffff);
        let a = rand_id(rng, 7);
        let b = rand_id(rng, 7);
        let n1 = rng.random_range(0u32..9999);
        let n2 = rng.random_range(0u32..9999);
        junk.push_str(&format!(
            "if(0x{pred:x}<0){{let _{a}=[{n1},{n2}];let _{b}=function(x,y){{return x^y^{n1};}};}}"
        ));
    }
    let result = format!("{junk}{js}");
    parses_ok(&result).then_some(result)
}

/// Conservative control-flow flattening: if the program body is a sequence of
/// simple expression statements (no declarations / control flow), reorder them
/// into a shuffled `switch` dispatcher driven by a sequential order array, which
/// preserves execution order while obscuring linear flow. Otherwise no-op.
fn cff_pass(js: &str, rng: &mut StdRng) -> Option<String> {
    use oxc::ast::ast::Statement;

    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, js, SourceType::cjs()).parse();
    if ret.panicked || !ret.diagnostics.is_empty() {
        return None;
    }

    // Only flatten when every top-level statement is a plain expression
    // statement - anything else (declarations, returns, loops) is unsafe to
    // move into switch cases due to hoisting/scoping.
    let body = &ret.program.body;
    if body.len() < 3 || !body.iter().all(|s| matches!(s, Statement::ExpressionStatement(_))) {
        return None;
    }

    let n = body.len();
    let mut fragments: Vec<String> = Vec::with_capacity(n);
    for stmt in body {
        let span = match stmt {
            Statement::ExpressionStatement(e) => e.span,
            _ => return None,
        };
        let mut frag = js[span.start as usize..span.end as usize].trim().to_string();
        if !frag.ends_with(';') {
            frag.push(';');
        }
        fragments.push(frag);
    }

    // Shuffled case labels; the order array lists them in execution order.
    let mut labels: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        let j = rng.random_range(0..=i);
        labels.swap(i, j);
    }
    let order = rand_id(rng, 5);
    let ptr = rand_id(rng, 5);

    // Fragment `i` (execution position `i`) lives under `case labels[i]`. The
    // order array selects labels[0], labels[1], ... so cases fire in original
    // sequence despite the shuffled case layout.
    let mut cases = String::new();
    for (exec_idx, label) in labels.iter().enumerate() {
        cases.push_str(&format!("case {label}:{}break;", fragments[exec_idx]));
    }
    let order_list = labels.iter().map(usize::to_string).collect::<Vec<_>>().join(",");

    let result = format!(
        "var _{order}=[{order_list}],_{ptr}=0;while(_{ptr}<_{order}.length){{switch(_{order}[_{ptr}++]){{{cases}}}}}"
    );
    parses_ok(&result).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn cfg() -> ObfuscationConfig {
        ObfuscationConfig {
            seed: Some(1),
            js_ast: true,
            ..Default::default()
        }
    }

    #[test]
    fn mangle_minify_roundtrips_parse() {
        let mut c = cfg();
        c.mangle_identifiers = true;
        let mut rng = StdRng::seed_from_u64(1);
        let src = "function add(a, b) { const sum = a + b; return sum; } add(1, 2);";
        let out = transform(src, &SymbolMap::new(Some(1)), &c, &mut rng).unwrap();
        assert!(parses_ok(&out));
    }

    #[test]
    fn string_array_hides_literals_and_reparses() {
        let mut rng = StdRng::seed_from_u64(1);
        let src = r#"var a="hello world"; var b="second string"; console.log(a,b);"#;
        let out = string_array_pass(src, &mut rng).unwrap();
        assert!(!out.contains("hello world"));
        assert!(out.contains("atob"));
        assert!(parses_ok(&out));
    }

    #[test]
    fn string_array_skips_property_keys_and_directives() {
        // "use strict" directive and the property key "k" must survive verbatim.
        let mut rng = StdRng::seed_from_u64(2);
        let src = r#""use strict"; var o = { "k": "v1", m: "v2" }; foo(o);"#;
        let out = string_array_pass(src, &mut rng).unwrap();
        assert!(out.contains("\"use strict\""));
        assert!(out.contains("\"k\""));
        assert!(parses_ok(&out));
    }

    #[test]
    fn dead_code_is_always_false_and_valid() {
        let mut rng = StdRng::seed_from_u64(3);
        let out = dead_code_pass("var x=1;foo(x);", 0.5, &mut rng).unwrap();
        assert!(out.contains("if(0x"));
        assert!(parses_ok(&out));
    }
}
