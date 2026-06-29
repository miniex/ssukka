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
use std::collections::{HashMap, HashSet};

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

    // mangle + minify; None here means unparsable input, so the caller falls back.
    code = mangle_and_print(&code, config)?;

    // Relabel the mangler's debug slots to misleading dictionary names.
    if config.poison_names {
        if let Some(next) = poison_pass(&code, rng) {
            code = next;
        }
    }

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

        if config.mangle_identifiers || config.poison_names {
            // top_level: false keeps globals/top-level functions intact, since
            // they may be referenced from other inline scripts or HTML handlers.
            // Poison mode emits debug `slot_N` names for the relabel pass below.
            let mangled = Mangler::new()
                .with_options(MangleOptions {
                    top_level: Some(false),
                    debug: config.poison_names,
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
/// directives (`"use strict"`), and import/export sources, which are not
/// expression nodes, are left untouched. Returns `None` if there is nothing to
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
    // statement; anything else (declarations, returns, loops) is unsafe to
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

/// Plausible-but-arbitrary names. Renaming locals to these (not base54 `e,t,n`)
/// anchors an LLM cleanup pass on misleading names it keeps rather than re-derives.
#[rustfmt::skip]
const POISON_WORDS: &[&str] = &[
    "handler", "buffer", "context", "payload", "session", "adapter", "cursor", "factory",
    "provider", "wrapper", "builder", "parser", "loader", "worker", "record", "registry",
    "manager", "sentinel", "beacon", "anchor", "harbor", "monitor", "tracker", "sampler",
    "mapper", "reducer", "emitter", "listener", "dispatcher", "scheduler", "validator",
    "formatter", "resolver", "collector", "aggregator", "broker", "courier", "warden",
    "scout", "ranger", "pilot", "envoy", "herald", "keeper", "weaver", "forge", "lantern",
    "compass", "ledger", "satchel", "beacon2", "marshal", "curator", "steward", "porter",
    "drifter", "glider", "mariner", "nomad", "ember", "cinder", "willow", "cedar", "harbor2",
    "meadow", "thicket", "hollow", "quarry", "summit", "delta", "vertex", "lattice", "prism",
    "cobalt", "amber", "onyx", "quartz", "basalt", "zephyr", "mistral", "monsoon",
];

/// `(start, end, slot index)` of one `slot_N` identifier occurrence.
type SlotSpan = (usize, usize, u32);

/// Rename the mangler's `slot_N` debug names to misleading dictionary words.
///
/// Each distinct slot gets a distinct word absent elsewhere in the program, so
/// nothing is shadowed; the pool is shuffled per build, overflow uses suffixes.
/// `None` if there is nothing to do or the result would not re-parse.
fn poison_pass(js: &str, rng: &mut StdRng) -> Option<String> {
    let (slots, used) = collect_slots_and_used(js)?;
    if slots.is_empty() {
        return None;
    }

    let mut pool: Vec<&str> = POISON_WORDS.to_vec();
    for i in (1..pool.len()).rev() {
        let j = rng.random_range(0..=i);
        pool.swap(i, j);
    }

    let mut indices: Vec<u32> = slots.iter().map(|&(_, _, i)| i).collect();
    indices.sort_unstable();
    indices.dedup();

    let mut names: HashMap<u32, String> = HashMap::new();
    let mut taken: HashSet<String> = HashSet::new();
    let mut pool_iter = pool.into_iter();
    let mut overflow = 0u32;
    for idx in indices {
        let name = pool_iter
            .by_ref()
            .find(|w| !used.contains(*w) && !taken.contains(*w))
            .map(str::to_owned)
            .unwrap_or_else(|| loop {
                overflow += 1;
                let cand = format!("handler{overflow}");
                if !used.contains(&cand) && !taken.contains(&cand) {
                    break cand;
                }
            });
        taken.insert(name.clone());
        names.insert(idx, name);
    }

    // Splice from the end so earlier byte offsets stay valid.
    let mut out = js.to_owned();
    let mut spans = slots;
    spans.sort_by_key(|&(start, _, _)| start);
    for (start, end, idx) in spans.iter().rev() {
        out.replace_range(*start..*end, &names[idx]);
    }
    parses_ok(&out).then_some(out)
}

/// Collect every `slot_N` identifier span, plus all other identifier names
/// (globals, members, labels, kept bindings) so poison names can avoid them.
fn collect_slots_and_used(js: &str) -> Option<(Vec<SlotSpan>, HashSet<String>)> {
    use oxc::ast::ast::{BindingIdentifier, IdentifierName, IdentifierReference, LabelIdentifier};
    use oxc::ast_visit::Visit;

    fn slot_index(name: &str) -> Option<u32> {
        name.strip_prefix("slot_").and_then(|n| n.parse::<u32>().ok())
    }

    struct Collector {
        slots: Vec<SlotSpan>,
        used: HashSet<String>,
    }
    impl<'a> Visit<'a> for Collector {
        fn visit_binding_identifier(&mut self, id: &BindingIdentifier<'a>) {
            match slot_index(&id.name) {
                Some(i) => self.slots.push((id.span.start as usize, id.span.end as usize, i)),
                None => {
                    self.used.insert(id.name.to_string());
                },
            }
        }
        fn visit_identifier_reference(&mut self, id: &IdentifierReference<'a>) {
            match slot_index(&id.name) {
                Some(i) => self.slots.push((id.span.start as usize, id.span.end as usize, i)),
                None => {
                    self.used.insert(id.name.to_string());
                },
            }
        }
        fn visit_identifier_name(&mut self, id: &IdentifierName<'a>) {
            self.used.insert(id.name.to_string());
        }
        fn visit_label_identifier(&mut self, id: &LabelIdentifier<'a>) {
            self.used.insert(id.name.to_string());
        }
    }

    for source_type in [SourceType::cjs(), SourceType::mjs()] {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, js, source_type).parse();
        if ret.panicked || !ret.diagnostics.is_empty() {
            continue;
        }
        let mut c = Collector {
            slots: Vec::new(),
            used: HashSet::new(),
        };
        c.visit_program(&ret.program);
        return Some((c.slots, c.used));
    }
    None
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
    fn poison_names_relabels_locals_without_slots_or_shadowing() {
        let mut c = cfg();
        c.poison_names = true;
        let mut rng = StdRng::seed_from_u64(7);
        let src = "function add(a, b) { const sum = a + b; return console.log(sum); } add(1, 2);";
        let out = transform(src, &SymbolMap::new(Some(7)), &c, &mut rng).unwrap();
        assert!(parses_ok(&out), "poisoned output must parse: {out}");
        assert!(!out.contains("slot_"), "debug slot names must be relabeled: {out}");
        // Globals/members must survive (never chosen as poison targets).
        assert!(out.contains("console"), "global must remain: {out}");
        assert!(
            POISON_WORDS.iter().any(|w| out.contains(w)),
            "a poison name should appear: {out}"
        );
    }

    #[test]
    fn poison_names_does_not_collide_with_program_identifiers() {
        // A global named like a poison word must not be shadowed: the pass skips
        // any word already present in the program.
        let mut c = cfg();
        c.poison_names = true;
        c.minify_js = false;
        let mut rng = StdRng::seed_from_u64(11);
        let src = "function f(x){var y=x+1;return handler(y);}f(handler);";
        let out = transform(src, &SymbolMap::new(Some(11)), &c, &mut rng).unwrap();
        assert!(parses_ok(&out), "must parse: {out}");
        // `handler` is a referenced global, so no local may be renamed to it.
        assert!(out.contains("handler"), "{out}");
    }

    #[test]
    fn dead_code_is_always_false_and_valid() {
        let mut rng = StdRng::seed_from_u64(3);
        let out = dead_code_pass("var x=1;foo(x);", 0.5, &mut rng).unwrap();
        assert!(out.contains("if(0x"));
        assert!(parses_ok(&out));
    }
}
