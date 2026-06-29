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
    // Last, so the canary's emitted source is exactly what runs (not re-printed).
    if config.self_defending {
        if let Some(next) = self_defending_pass(&code, rng) {
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

/// Random lowercase identifier suffix.
fn rand_id(rng: &mut StdRng, len: usize) -> String {
    const CH: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
    (0..len).map(|_| CH[rng.random_range(0..CH.len())] as char).collect()
}

/// Hoist string-literal *expressions* into a per-build character pool decoded by
/// index, so there is no `atob` / `fromCharCode` / `TextDecoder` and no base64
/// array for a hook-based deobfuscator to latch onto. A tool that *executes* the
/// decoder still recovers the strings (the documented threat boundary).
///
/// Only `StringLiteral` expressions are rewritten (property keys, directives, and
/// import sources stay untouched). `None` if nothing to do or it won't re-parse.
fn string_array_pass(js: &str, rng: &mut StdRng) -> Option<String> {
    let spans = collect_string_expr_spans(js)?;
    if spans.len() < 2 {
        return None;
    }

    // Distinct UTF-16 code units across all values, shuffled per build.
    let mut seen = HashSet::new();
    let mut pool: Vec<u16> = Vec::new();
    for (_, _, raw) in &spans {
        for u in raw.encode_utf16() {
            if seen.insert(u) {
                pool.push(u);
            }
        }
    }
    for i in (1..pool.len()).rev() {
        let j = rng.random_range(0..=i);
        pool.swap(i, j);
    }
    let pool_idx: HashMap<u16, usize> = pool.iter().enumerate().map(|(i, u)| (*u, i)).collect();

    let offset = rng.random_range(1u32..=9999) as usize;
    let pool_var = format!("_{}", rand_id(rng, 6));
    let dec = format!("_{}", rand_id(rng, 6));

    // Splice from the end so earlier byte offsets stay valid.
    let mut out = js.to_owned();
    for (start, end, raw) in spans.iter().rev() {
        let indices: Vec<String> = raw
            .encode_utf16()
            .map(|u| (pool_idx[&u] + offset).to_string())
            .collect();
        out.replace_range(*start..*end, &format!("{dec}([{}])", indices.join(",")));
    }

    // Pool as a string literal; every unit escaped so any content is safe.
    let mut pool_lit = String::with_capacity(pool.len() * 6 + 2);
    pool_lit.push('"');
    for u in &pool {
        pool_lit.push_str(&format!("\\u{u:04x}"));
    }
    pool_lit.push('"');

    // Vary the decoder body per build so it isn't a fixed signature.
    let body = match rng.random_range(0u8..3) {
        0 => format!("var s=\"\";for(var k=0;k<a.length;k++)s+={pool_var}[a[k]-{offset}];return s;"),
        1 => format!("return a.map(function(x){{return {pool_var}[x-{offset}];}}).join(\"\");"),
        _ => format!("return a.reduce(function(s,x){{return s+{pool_var}[x-{offset}];}},\"\");"),
    };
    let prelude = format!("const {pool_var}={pool_lit};function {dec}(a){{{body}}}");

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
/// A provably-always-false predicate. The shape varies per call so the emitted
/// guard isn't a fixed signature; every form is false for *any* literal.
fn opaque_false(rng: &mut StdRng) -> String {
    let n = rng.random_range(0x1000u32..=0xffff);
    let m = rng.random_range(1u32..9999);
    match rng.random_range(0u8..4) {
        0 => format!("0x{n:x}<0"),     // a positive literal is never < 0
        1 => format!("(0x{n:x}&0)>0"), // x & 0 == 0
        2 => format!("({n}^{n})>{m}"), // x ^ x == 0, never > m>=1
        _ => format!("!({n}|1)"),      // x | 1 is truthy, so !(..) is false
    }
}

/// A self-contained junk statement that never runs (shape varies per call).
fn junk_body(rng: &mut StdRng) -> String {
    let a = rand_id(rng, 7);
    let n1 = rng.random_range(0u32..9999);
    let n2 = rng.random_range(0u32..9999);
    match rng.random_range(0u8..3) {
        0 => {
            let b = rand_id(rng, 7);
            format!("let _{a}=[{n1},{n2}];let _{b}=function(x,y){{return x^y^{n1};}};")
        },
        1 => format!("var _{a}={n1};while(_{a}>0){{_{a}--;}}"),
        _ => format!("const _{a}={{k:{n1},v:{n2}}};"),
    }
}

fn dead_code_pass(js: &str, threshold: f32, rng: &mut StdRng) -> Option<String> {
    if threshold <= 0.0 {
        return None;
    }
    let count = ((threshold * 4.0).ceil() as usize).clamp(1, 6);
    let mut junk = String::new();
    for _ in 0..count {
        junk.push_str(&format!("if({}){{{}}}", opaque_false(rng), junk_body(rng)));
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

/// Rolling hash mirrored from the injected `h(s)`: `v = (v*31 + code) >>> 0`.
fn js_hash(s: &str) -> u32 {
    let mut v: u32 = 0;
    for u in s.encode_utf16() {
        v = v.wrapping_mul(31).wrapping_add(u32::from(u));
    }
    v
}

/// Inject a self-check: a canary function whose `toString()` is hashed at load
/// and compared to a build-time hash; if the script was beautified, the hash
/// differs and `console` is stubbed out. Emitted verbatim so the runtime source
/// matches the hash. Deters casual beautify-and-run; stripping the guard defeats it.
fn self_defending_pass(js: &str, rng: &mut StdRng) -> Option<String> {
    let kname = format!("_{}", rand_id(rng, 7));
    let gname = format!("_{}", rand_id(rng, 7));
    let hname = format!("_{}", rand_id(rng, 7));
    let token = rng.random_range(100_000u32..=999_999);

    // The canary's source, exactly as emitted; `kname.toString()` must hash to this.
    let canary = format!("function {kname}(){{return {token};}}");
    let expected = js_hash(&canary);

    let guard = format!(
        "function {hname}(s){{var v=0;for(var i=0;i<s.length;i++)v=(v*31+s.charCodeAt(i))>>>0;return v;}}\
function {gname}(){{if({hname}({kname}.toString())!=={expected}){{console.log=console.info=console.warn=function(){{}};}}}}\
{gname}();"
    );

    let result = format!("{canary}{guard}{js}");
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
        assert!(!out.contains("second string"));
        // Anti-hook: no standard decode primitives for a deobfuscator to latch onto.
        assert!(!out.contains("atob"));
        assert!(!out.contains("fromCharCode"));
        assert!(!out.contains("TextDecoder"));
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
    fn self_defending_embeds_matching_canary_hash() {
        let mut rng = StdRng::seed_from_u64(5);
        let out = self_defending_pass("foo();", &mut rng).unwrap();
        assert!(parses_ok(&out));
        // The first function is the canary; its source must hash to the embedded
        // expected value, so the guard does not fire on the verbatim output.
        let start = out.find("function ").unwrap();
        let end = out[start..].find('}').unwrap() + start + 1;
        let expected = js_hash(&out[start..end]);
        assert!(out.contains(&format!("!=={expected}")), "hash must match canary: {out}");
    }

    #[test]
    fn dead_code_variants_parse_and_keep_original() {
        // Exercise every predicate/body shape; all must parse and leave the
        // original code intact (the junk is guarded by a false predicate).
        for seed in 0..16u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            let out = dead_code_pass("var x=1;foo(x);", 0.9, &mut rng).unwrap();
            assert!(out.contains("if("), "{out}");
            assert!(out.contains("foo(x)"), "original preserved: {out}");
            assert!(parses_ok(&out), "{out}");
        }
    }
}
