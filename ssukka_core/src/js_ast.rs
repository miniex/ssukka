//! AST-based JS obfuscation via [oxc](https://github.com/oxc-project/oxc):
//! local identifier mangling, minification, string arrays, dead code, and
//! control-flow flattening/virtualization.
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

    // Object keys -> computed string keys, before the string array so it can
    // encode them.
    if config.property_keys {
        if let Some(next) = property_keys_pass(&code) {
            code = next;
        }
    }

    // String arrays before codegen, which may turn literals into templates.
    if config.js_string_encoding == crate::config::JsStringEncoding::Array {
        if let Some(next) = string_array_pass(&code, &config.reserved_strings, config.string_array_threshold, rng) {
            code = next;
        }
    }

    // mangle + minify; None here means unparsable input, so the caller falls back.
    code = mangle_and_print(&code, config)?;

    // After codegen (the printer would fold it back) and before self-defending
    // (the canary hash must cover the final form).
    if config.mba {
        if let Some(next) = mba_pass(&code, rng) {
            code = next;
        }
    }

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
    // Virtualization first; it takes the same eligible blocks as flattening, so
    // any leftover for cff_pass will already contain control flow and be skipped.
    if config.virtualize {
        if let Some(next) = vm_pass(&code, rng) {
            code = next;
        }
    }
    if config.control_flow_flattening {
        if let Some(next) = cff_pass(&code, rng) {
            code = next;
        }
    }
    if config.opaque_predicates {
        if let Some(next) = opaque_branch_pass(&code, rng) {
            code = next;
        }
    }
    if !config.domain_lock.is_empty() || config.lock_expiry_secs.is_some() {
        if let Some(next) = lock_pass(&code, &config.domain_lock, config.lock_expiry_secs, rng) {
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
fn string_array_pass(js: &str, reserved: &[String], threshold: f32, rng: &mut StdRng) -> Option<String> {
    let mut spans = collect_string_expr_spans(js)?;
    // Whitelist: keep reserved strings readable.
    if !reserved.is_empty() {
        spans.retain(|(_, _, v)| !reserved.iter().any(|r| r == v));
    }
    // Threshold: encode only a fraction (rng untouched at 1.0 to keep default output).
    if threshold < 1.0 {
        spans.retain(|_| (rng.random_range(0..1000) as f32) < threshold * 1000.0);
    }
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
    use oxc::ast::ast::{Expression, ObjectProperty, PropertyKey};
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

        // Encode the value always; the key only when computed (a computed key is
        // an expression position, a static key can't become a call).
        fn visit_object_property(&mut self, prop: &ObjectProperty<'a>) {
            if prop.computed {
                if let Some(e) = prop.key.as_expression() {
                    self.visit_expression(e);
                }
            }
            self.visit_expression(&prop.value);
        }

        // Other property keys (class members, static object keys) stay readable so
        // a key never becomes a call expression.
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

/// Convert safe object-literal keys (`{foo: v}`) into computed string keys
/// (`{["foo"]: v}`) so the string array can encode them. Skips methods,
/// getters/setters, shorthand, computed, numeric, and `__proto__` (a computed key
/// drops its prototype-setting form). `None` if nothing to convert or it won't re-parse.
fn property_keys_pass(js: &str) -> Option<String> {
    use oxc::ast::ast::{ObjectProperty, PropertyKey, PropertyKind};
    use oxc::ast_visit::{walk, Visit};

    struct Collector {
        repl: Vec<(usize, usize, String)>,
    }
    impl<'a> Visit<'a> for Collector {
        fn visit_object_property(&mut self, prop: &ObjectProperty<'a>) {
            if matches!(prop.kind, PropertyKind::Init) && !prop.method && !prop.shorthand && !prop.computed {
                let key = match &prop.key {
                    PropertyKey::StaticIdentifier(id) => Some((id.span, id.name.as_str())),
                    PropertyKey::StringLiteral(lit) => Some((lit.span, lit.value.as_str())),
                    _ => None,
                };
                if let Some((span, name)) = key {
                    if name != "__proto__" {
                        let mut rep = String::from("[\"");
                        for c in name.chars() {
                            match c {
                                '\\' => rep.push_str("\\\\"),
                                '"' => rep.push_str("\\\""),
                                '\n' => rep.push_str("\\n"),
                                _ => rep.push(c),
                            }
                        }
                        rep.push_str("\"]");
                        self.repl.push((span.start as usize, span.end as usize, rep));
                    }
                }
            }
            walk::walk_object_property(self, prop);
        }
    }

    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, js, SourceType::cjs()).parse();
    if ret.panicked || !ret.diagnostics.is_empty() {
        return None;
    }
    let mut c = Collector { repl: Vec::new() };
    c.visit_program(&ret.program);
    if c.repl.is_empty() {
        return None;
    }
    c.repl.sort_by_key(|&(start, _, _)| start);
    // Splice from the end so earlier byte offsets stay valid.
    let mut out = js.to_owned();
    for (start, end, rep) in c.repl.iter().rev() {
        out.replace_range(*start..*end, rep);
    }
    parses_ok(&out).then_some(out)
}

/// MBA literal bound: i32::MAX keeps operands and bitwise results non-negative
/// int32, so the identities stay exact under JS.
const MBA_MAX: u64 = 0x7fff_ffff;

/// Replace integer literals with equivalent mixed boolean-arithmetic - exact in
/// JS for int32-range operands: `a+b == (a^b)+2*(a&b) == (a|b)+(a&b)`. Only
/// non-negative ints in `[2, MBA_MAX]` (numeric keys / float / out-of-range left
/// alone). `None` if there's nothing to do or it won't re-parse.
fn mba_pass(js: &str, rng: &mut StdRng) -> Option<String> {
    let mut spans = collect_int_literal_spans(js)?;
    if spans.is_empty() {
        return None;
    }
    spans.sort_by_key(|&(start, _, _)| start);
    // Splice from the end so earlier byte offsets stay valid.
    let mut out = js.to_owned();
    for (start, end, n) in spans.iter().rev() {
        out.replace_range(*start..*end, &mba_encode(*n, rng));
    }
    parses_ok(&out).then_some(out)
}

/// One MBA layer for `n` (>= 2): split `n = a + b` and emit a fully-parenthesized
/// identity (so JS operator precedence is irrelevant).
fn mba_encode(n: u64, rng: &mut StdRng) -> String {
    let a = rng.random_range(0..=n);
    let b = n - a;
    match rng.random_range(0u8..2) {
        0 => format!("(({a}^{b})+2*({a}&{b}))"),
        _ => format!("(({a}|{b})+({a}&{b}))"),
    }
}

/// `(start, end, value)` for every non-negative integer literal in `[2, MBA_MAX]`
/// in expression position. Numeric keys are skipped (can't be an expression);
/// their values are still visited.
fn collect_int_literal_spans(js: &str) -> Option<Vec<(usize, usize, u64)>> {
    use oxc::ast::ast::{Expression, PropertyKey};
    use oxc::ast_visit::{walk, Visit};

    struct Collector {
        spans: Vec<(usize, usize, u64)>,
    }
    impl<'a> Visit<'a> for Collector {
        fn visit_expression(&mut self, expr: &Expression<'a>) {
            if let Expression::NumericLiteral(lit) = expr {
                let v = lit.value;
                if v.is_finite() && v.fract() == 0.0 && (2.0..=MBA_MAX as f64).contains(&v) {
                    self.spans
                        .push((lit.span.start as usize, lit.span.end as usize, v as u64));
                }
            }
            walk::walk_expression(self, expr);
        }
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

/// A predicate true for ALL non-negative int32 operands (not just the chosen
/// constants), so the guarded code always runs. Bitwise identities.
fn opaque_true(rng: &mut StdRng) -> String {
    let a = rng.random_range(0..=MBA_MAX);
    let b = rng.random_range(0..=MBA_MAX);
    match rng.random_range(0u8..3) {
        0 => format!("(({a}^{b})===(({a}|{b})-({a}&{b})))"), // a^b == (a|b)-(a&b)
        1 => format!("(({a}&{b})<=({a}|{b}))"),              // a&b <= a|b
        _ => format!("(({a}|{b})>={a})"),                    // a|b >= a
    }
}

/// Wrap top-level expression statements in an always-true guard
/// (`if(<opaque>){ stmt }`). Only expression statements are wrapped - wrapping
/// declarations would change block scoping, and string-literal directives must
/// stay in place. `None` if there's nothing to wrap or it won't re-parse.
fn opaque_branch_pass(js: &str, rng: &mut StdRng) -> Option<String> {
    use oxc::ast::ast::{Expression, Statement};

    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, js, SourceType::cjs()).parse();
    if ret.panicked || !ret.diagnostics.is_empty() {
        return None;
    }
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for stmt in &ret.program.body {
        if let Statement::ExpressionStatement(es) = stmt {
            if matches!(es.expression, Expression::StringLiteral(_)) {
                continue; // leave "use strict" and other directives in place
            }
            spans.push((es.span.start as usize, es.span.end as usize));
        }
    }
    if spans.is_empty() {
        return None;
    }
    // Splice from the end so earlier byte offsets stay valid.
    let mut out = js.to_owned();
    for (start, end) in spans.iter().rev() {
        let wrapped = format!("if({}){{{}}}", opaque_true(rng), &js[*start..*end]);
        out.replace_range(*start..*end, &wrapped);
    }
    parses_ok(&out).then_some(out)
}

/// Prepend an execution lock: a guard that crashes the script (unbounded
/// recursion -> `RangeError`) off an allowed domain or past an expiry, and is a
/// no-op otherwise. Random identifiers avoid a fixed signature. `None` if it
/// won't re-parse.
fn lock_pass(js: &str, domains: &[String], expiry_secs: Option<u64>, rng: &mut StdRng) -> Option<String> {
    let mut conds: Vec<String> = Vec::new();
    if !domains.is_empty() {
        let h = format!("_{}", rand_id(rng, 5));
        let d = format!("_{}", rand_id(rng, 5));
        let list = domains
            .iter()
            .map(|x| format!("\"{}\"", x.replace('\\', "\\\\").replace('"', "\\\"")))
            .collect::<Vec<_>>()
            .join(",");
        conds.push(format!(
            "(function(){{var {h}=(typeof location!=='undefined'&&location.hostname)||'';\
return![{list}].some(function({d}){{return {h}==={d}||{h}.endsWith('.'+{d});}});}})()"
        ));
    }
    if let Some(secs) = expiry_secs {
        conds.push(format!("(typeof Date!=='undefined'&&Date.now()>{secs}000)"));
    }
    if conds.is_empty() {
        return None;
    }
    let f = format!("_{}", rand_id(rng, 6));
    let result = format!("if({}){{(function {f}(){{{f}();}})();}}{js}", conds.join("||"));
    parses_ok(&result).then_some(result)
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

/// Shared block flattener for [`cff_pass`] and [`vm_pass`]: find straight-line
/// statement sequences (top-level or a function/arrow block body) and hand each to
/// `render`. Correct-by-construction: a block is taken only if every statement is an
/// expression statement or a simple `var` decl (hoisted) - no control flow,
/// `let`/`const`, or declarations to preserve, and `this`/`arguments` stay put.
/// Chosen blocks don't overlap. `None` if nothing matches or it won't re-parse.
fn flatten_blocks(
    js: &str,
    rng: &mut StdRng,
    render: fn(&[String], &[String], &mut StdRng) -> String,
) -> Option<String> {
    use oxc::ast::ast::{BindingPattern, FunctionBody, Program, Statement, VariableDeclarationKind};
    use oxc::ast_visit::{walk, Visit};
    use oxc::span::GetSpan;

    /// `(hoisted var names, ordered executable fragments)`, or `None` if any
    /// statement is ineligible or there are fewer than two fragments.
    fn block_units(js: &str, stmts: &[Statement]) -> Option<(Vec<String>, Vec<String>)> {
        let slice = |s: u32, e: u32| js[s as usize..e as usize].trim().to_string();
        let mut hoist: Vec<String> = Vec::new();
        let mut frags: Vec<String> = Vec::new();
        for stmt in stmts {
            match stmt {
                Statement::ExpressionStatement(e) => {
                    let mut f = slice(e.span.start, e.span.end);
                    if !f.ends_with(';') {
                        f.push(';');
                    }
                    frags.push(f);
                },
                Statement::VariableDeclaration(v) if matches!(v.kind, VariableDeclarationKind::Var) => {
                    for d in &v.declarations {
                        let name = match &d.id {
                            BindingPattern::BindingIdentifier(bi) => bi.name.to_string(),
                            _ => return None, // destructuring pattern: bail
                        };
                        if !hoist.contains(&name) {
                            hoist.push(name.clone());
                        }
                        if let Some(init) = &d.init {
                            let s = init.span();
                            frags.push(format!("{name}={};", slice(s.start, s.end)));
                        }
                    }
                },
                _ => return None,
            }
        }
        (frags.len() >= 2).then_some((hoist, frags))
    }

    type Cand = (usize, usize, Vec<String>, Vec<String>);
    struct Collector<'s> {
        js: &'s str,
        cands: Vec<Cand>,
    }
    impl Collector<'_> {
        fn consider(&mut self, stmts: &[Statement]) {
            let (Some(first), Some(last)) = (stmts.first(), stmts.last()) else {
                return;
            };
            if let Some((hoist, frags)) = block_units(self.js, stmts) {
                self.cands
                    .push((first.span().start as usize, last.span().end as usize, hoist, frags));
            }
        }
    }
    impl<'a> Visit<'a> for Collector<'_> {
        fn visit_program(&mut self, it: &Program<'a>) {
            self.consider(&it.body);
            walk::walk_program(self, it);
        }
        fn visit_function_body(&mut self, it: &FunctionBody<'a>) {
            // Concise arrow bodies have one (implicit-return) statement, so they
            // never reach two fragments and are left intact.
            self.consider(&it.statements);
            walk::walk_function_body(self, it);
        }
    }

    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, js, SourceType::cjs()).parse();
    if ret.panicked || !ret.diagnostics.is_empty() {
        return None;
    }
    let mut c = Collector { js, cands: Vec::new() };
    c.visit_program(&ret.program);

    // Pick non-overlapping blocks, innermost (latest start) first.
    c.cands.sort_by_key(|&(start, ..)| std::cmp::Reverse(start));
    let mut selected: Vec<Cand> = Vec::new();
    for cand in c.cands {
        if !selected.iter().any(|s| cand.0 < s.1 && cand.1 > s.0) {
            selected.push(cand);
        }
    }
    if selected.is_empty() {
        return None;
    }

    // Splice from the end (selected is start-descending) so offsets stay valid.
    let mut out = js.to_owned();
    for (start, end, hoist, frags) in &selected {
        out.replace_range(*start..*end, &render(hoist, frags, rng));
    }
    parses_ok(&out).then_some(out)
}

/// Control-flow flattening: rewrite eligible blocks into a `while`/`switch` state
/// machine (shuffled cases, random state variable), hiding the linear order.
fn cff_pass(js: &str, rng: &mut StdRng) -> Option<String> {
    flatten_blocks(js, rng, cff_dispatcher)
}

/// Control-flow virtualization: a stronger alternative to [`cff_pass`] that runs
/// eligible blocks through a bytecode interpreter (see [`vm_render`]) so the order
/// lives in data, not JS control flow; arrows keep `this`/`arguments`.
fn vm_pass(js: &str, rng: &mut StdRng) -> Option<String> {
    flatten_blocks(js, rng, vm_render)
}

/// `var a,b;` hoist prefix for a flattened block (empty when there is none).
fn hoist_decl(hoist: &[String]) -> String {
    if hoist.is_empty() {
        String::new()
    } else {
        format!("var {};", hoist.join(","))
    }
}

/// Build the `while`/`switch` state machine for one flattened block: fragments
/// fire in original order through a shuffled, randomly-labelled state, with the
/// `var` bindings hoisted ahead of the loop.
fn cff_dispatcher(hoist: &[String], frags: &[String], rng: &mut StdRng) -> String {
    let n = frags.len();
    // n case labels + 1 terminal, all distinct.
    let mut labels: Vec<u32> = Vec::with_capacity(n + 1);
    let mut seen = HashSet::new();
    while labels.len() < n + 1 {
        let x = rng.random_range(1000u32..=9_999_999);
        if seen.insert(x) {
            labels.push(x);
        }
    }
    let st = format!("_{}", rand_id(rng, 5));
    let done = labels[n];

    let mut order: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        let j = rng.random_range(0..=i);
        order.swap(i, j);
    }
    let mut cases = String::new();
    for &p in &order {
        cases.push_str(&format!("case {}:{}{st}={};break;", labels[p], frags[p], labels[p + 1]));
    }
    format!(
        "{}var {st}={};while({st}!=={done}){{switch({st}){{{cases}}}}}",
        hoist_decl(hoist),
        labels[0]
    )
}

/// Build the virtual machine for one flattened block: an op table of arrow thunks
/// stored in shuffled order, an XOR-encoded bytecode array listing them in
/// execution order, and a loop that decodes each byte and calls the thunk.
fn vm_render(hoist: &[String], frags: &[String], rng: &mut StdRng) -> String {
    let n = frags.len();
    // slot[exec position] = storage index in the op table (a random permutation).
    let mut slot: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        slot.swap(i, rng.random_range(0..=i));
    }
    let mut store_to_exec = vec![0usize; n];
    for (p, &s) in slot.iter().enumerate() {
        store_to_exec[s] = p;
    }

    let key = rng.random_range(1u32..=0xffff);
    let ops = (0..n)
        .map(|s| format!("()=>{{{}}}", frags[store_to_exec[s]]))
        .collect::<Vec<_>>()
        .join(",");
    let code = slot
        .iter()
        .map(|&s| (s as u32 ^ key).to_string())
        .collect::<Vec<_>>()
        .join(",");

    let vt = format!("_{}", rand_id(rng, 5));
    let bc = format!("_{}", rand_id(rng, 5));
    let pc = format!("_{}", rand_id(rng, 5));
    format!(
        "{}var {vt}=[{ops}],{bc}=[{code}];for(var {pc}=0;{pc}<{bc}.length;{pc}++){{{vt}[{bc}[{pc}]^{key}]();}}",
        hoist_decl(hoist)
    )
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
        let out = string_array_pass(src, &[], 1.0, &mut rng).unwrap();
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
        let out = string_array_pass(src, &[], 1.0, &mut rng).unwrap();
        assert!(out.contains("\"use strict\""));
        assert!(out.contains("\"k\""));
        assert!(parses_ok(&out));
    }

    #[test]
    fn string_array_keeps_reserved_strings_readable() {
        let mut rng = StdRng::seed_from_u64(1);
        let src = r#"var a="keepme"; var b="secret one"; var c="secret two"; f(a,b,c);"#;
        let out = string_array_pass(src, &["keepme".into()], 1.0, &mut rng).unwrap();
        assert!(out.contains("\"keepme\""), "reserved string must stay readable: {out}");
        assert!(!out.contains("secret one") && !out.contains("secret two"), "{out}");
        assert!(parses_ok(&out));
    }

    #[test]
    fn property_keys_become_computed_strings() {
        let out = property_keys_pass(r#"var o = { foo: 1, "bar": 2, [x]: 3, m() {}, __proto__: p };"#).unwrap();
        assert!(parses_ok(&out), "{out}");
        assert!(out.contains(r#"["foo"]"#), "identifier key -> computed string: {out}");
        assert!(out.contains(r#"["bar"]"#), "string key -> computed string: {out}");
        // computed key, method, and __proto__ are left untouched.
        assert!(
            out.contains("[x]") && out.contains("m()") && out.contains("__proto__:"),
            "{out}"
        );
    }

    #[test]
    fn property_keys_then_string_array_encodes_keys() {
        // With both passes, the converted keys are hoisted into the string array.
        let mut c = cfg();
        c.property_keys = true;
        c.js_string_encoding = crate::config::JsStringEncoding::Array;
        let mut rng = StdRng::seed_from_u64(4);
        let src = r#"var o = { secretKey: 1, other: 2 }; use(o);"#;
        let out = transform(src, &SymbolMap::new(Some(4)), &c, &mut rng).unwrap();
        assert!(parses_ok(&out), "{out}");
        assert!(!out.contains("secretKey"), "key name should be encoded away: {out}");
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

    #[test]
    fn mba_identities_are_exact() {
        // Both identities hold for every int32-range non-negative pair (u64 mirrors JS exactly here).
        for (a, b) in [
            (0u64, 0u64),
            (1, 0),
            (3, 2),
            (255, 1000),
            (0x7fff_ffff, 0),
            (123456, 654321),
        ] {
            assert_eq!((a ^ b) + 2 * (a & b), a + b);
            assert_eq!((a | b) + (a & b), a + b);
        }
    }

    #[test]
    fn mba_encode_evaluates_to_n() {
        for seed in 0..32u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            for &n in &[2u64, 7, 42, 255, 1000, 65535, 0x7fff_ffff] {
                let s = mba_encode(n, &mut rng);
                assert_eq!(eval_uint_expr(&s), n, "seed {seed}, n {n}, expr {s}");
            }
        }
    }

    #[test]
    fn mba_pass_rewrites_literals_and_reparses() {
        let mut c = cfg();
        c.mba = true;
        c.minify_js = false;
        let mut rng = StdRng::seed_from_u64(3);
        let src = "var a = 255; var b = a + 1000; foo(b, 42);";
        let out = transform(src, &SymbolMap::new(Some(3)), &c, &mut rng).unwrap();
        assert!(parses_ok(&out), "{out}");
        // MBA expressions present (bitwise ops appear), and the rewrite happened.
        assert!(
            out.contains('&') && (out.contains('^') || out.contains('|')),
            "expected MBA ops: {out}"
        );
    }

    #[test]
    fn opaque_true_identities_always_hold() {
        // The three predicate forms are true for ALL non-negative int32 pairs.
        for (a, b) in [(0u64, 0u64), (1, 2), (255, 1000), (0x7fff_ffff, 1), (123456, 654321)] {
            assert_eq!(a ^ b, (a | b) - (a & b));
            assert!((a & b) <= (a | b));
            assert!((a | b) >= a);
        }
    }

    #[test]
    fn lock_pass_emits_domain_and_expiry_guards() {
        let mut rng = StdRng::seed_from_u64(2);
        let out = lock_pass("foo();", &["example.com".into()], Some(1893456000), &mut rng).unwrap();
        assert!(parses_ok(&out), "{out}");
        assert!(out.contains("\"example.com\""), "allowed host listed: {out}");
        assert!(
            out.contains("location") && out.contains("endsWith"),
            "host check: {out}"
        );
        assert!(
            out.contains("Date.now()") && out.contains("1893456000000"),
            "expiry check (ms): {out}"
        );
        assert!(out.contains("foo();"), "original code preserved: {out}");
    }

    #[test]
    fn lock_pass_is_noop_without_targets() {
        let mut rng = StdRng::seed_from_u64(1);
        assert!(lock_pass("foo();", &[], None, &mut rng).is_none());
    }

    #[test]
    fn lock_pass_via_transform_keeps_code() {
        let mut c = cfg();
        c.domain_lock = vec!["example.com".into()];
        let mut rng = StdRng::seed_from_u64(3);
        let out = transform("var x=1;foo(x);", &SymbolMap::new(Some(3)), &c, &mut rng).unwrap();
        assert!(parses_ok(&out), "{out}");
        assert!(out.contains("example.com"), "{out}");
        assert!(out.contains("foo(x)"), "{out}");
    }

    #[test]
    fn opaque_branch_wraps_expr_statements_and_reparses() {
        let mut rng = StdRng::seed_from_u64(4);
        let out = opaque_branch_pass("foo();bar(x);", &mut rng).unwrap();
        assert!(parses_ok(&out), "{out}");
        assert!(out.contains("if("), "statements should be guarded: {out}");
        assert!(
            out.contains("foo()") && out.contains("bar(x)"),
            "originals preserved: {out}"
        );
    }

    #[test]
    fn opaque_branch_leaves_directives_and_declarations() {
        let mut rng = StdRng::seed_from_u64(8);
        let out = opaque_branch_pass(r#""use strict";var x=1;foo();"#, &mut rng).unwrap();
        assert!(parses_ok(&out), "{out}");
        // Directive prologue and declarations must not be wrapped.
        assert!(out.starts_with(r#""use strict";"#), "directive must stay first: {out}");
        assert!(out.contains("var x=1;"), "declaration must stay unwrapped: {out}");
        assert!(out.contains("if("), "the expression statement should be wrapped: {out}");
    }

    #[test]
    fn cff_flattens_straightline_function_body() {
        let mut rng = StdRng::seed_from_u64(3);
        let out = cff_pass("function r(){var a=2;var b=a*3;log(a);log(b);}", &mut rng).unwrap();
        assert!(parses_ok(&out), "{out}");
        assert!(
            out.contains("switch(") && out.contains("while("),
            "dispatcher emitted: {out}"
        );
        assert!(
            out.contains("var a,b") || (out.contains("var a") && out.contains("var b")),
            "vars hoisted: {out}"
        );
    }

    #[test]
    fn cff_bails_on_control_flow() {
        // An `if` makes the body ineligible, and the top level is just a function
        // declaration, so nothing flattens.
        let mut rng = StdRng::seed_from_u64(1);
        assert!(cff_pass("function r(){var a=1;if(a){log(a);}log(a);}", &mut rng).is_none());
    }

    #[test]
    fn vm_virtualizes_straightline_function_body() {
        let mut rng = StdRng::seed_from_u64(5);
        let out = vm_pass("function r(){var a=2;var b=a*3;log(a);log(b);}", &mut rng).unwrap();
        assert!(parses_ok(&out), "{out}");
        assert!(
            out.contains("=>{") && out.contains(".length;"),
            "op table + dispatch loop: {out}"
        );
        assert!(
            out.contains("var a,b") || (out.contains("var a") && out.contains("var b")),
            "vars hoisted: {out}"
        );
    }

    #[test]
    fn mba_skips_numeric_property_keys() {
        // `{ 5: x }` must stay a valid key, not become `{ (..): x }`.
        let mut rng = StdRng::seed_from_u64(1);
        let out = mba_pass("var o = { 255: 1 }; o[255];", &mut rng).unwrap();
        assert!(parses_ok(&out), "{out}");
        assert!(out.contains("255:"), "numeric key must survive verbatim: {out}");
        // The computed access `o[255]` is an expression position, so it is rewritten.
        assert!(out.contains("o[(("), "computed index should be MBA-encoded: {out}");
    }

    /// Evaluate an expression over `^ & | + *` and integers with JS operator
    /// precedence (`*` > `+` > `&` > `^` > `|`), using u64 (exact for the int32
    /// range mba_encode emits). Test-only mirror of JS arithmetic.
    fn eval_uint_expr(s: &str) -> u64 {
        let t: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
        let mut p = 0;
        let v = parse_bitor(&t, &mut p);
        assert_eq!(p, t.len(), "trailing tokens in {s}");
        v
    }
    fn parse_bitor(t: &[char], p: &mut usize) -> u64 {
        let mut v = parse_bitxor(t, p);
        while *p < t.len() && t[*p] == '|' {
            *p += 1;
            v |= parse_bitxor(t, p);
        }
        v
    }
    fn parse_bitxor(t: &[char], p: &mut usize) -> u64 {
        let mut v = parse_bitand(t, p);
        while *p < t.len() && t[*p] == '^' {
            *p += 1;
            v ^= parse_bitand(t, p);
        }
        v
    }
    fn parse_bitand(t: &[char], p: &mut usize) -> u64 {
        let mut v = parse_add(t, p);
        while *p < t.len() && t[*p] == '&' {
            *p += 1;
            v &= parse_add(t, p);
        }
        v
    }
    fn parse_add(t: &[char], p: &mut usize) -> u64 {
        let mut v = parse_mul(t, p);
        while *p < t.len() && t[*p] == '+' {
            *p += 1;
            v += parse_mul(t, p);
        }
        v
    }
    fn parse_mul(t: &[char], p: &mut usize) -> u64 {
        let mut v = parse_atom(t, p);
        while *p < t.len() && t[*p] == '*' {
            *p += 1;
            v *= parse_atom(t, p);
        }
        v
    }
    fn parse_atom(t: &[char], p: &mut usize) -> u64 {
        if t[*p] == '(' {
            *p += 1;
            let v = parse_bitor(t, p);
            assert_eq!(t[*p], ')');
            *p += 1;
            v
        } else {
            let start = *p;
            while *p < t.len() && t[*p].is_ascii_digit() {
                *p += 1;
            }
            t[start..*p].iter().collect::<String>().parse().unwrap()
        }
    }
}
