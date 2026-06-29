//! WebAssembly bindings (enabled with the `wasm` feature).
//!
//! Build for the browser/edge with:
//! ```sh
//! wasm-pack build --features wasm
//! # or
//! cargo build --release --target wasm32-unknown-unknown --features wasm
//! ```
//!
//! Because ssukka performs no I/O, the module runs anywhere WASM does
//! (Cloudflare Workers, browsers, Deno, wasmtime) with no network access.

use crate::config::JsStringEncoding;
use crate::Obfuscator;
use wasm_bindgen::prelude::*;

/// Obfuscate HTML with default settings (non-deterministic output).
#[wasm_bindgen]
pub fn obfuscate(html: &str) -> Result<String, JsValue> {
    crate::obfuscate(html).map_err(to_js)
}

/// Obfuscate HTML deterministically with an explicit seed.
#[wasm_bindgen]
pub fn obfuscate_seeded(html: &str, seed: u64) -> Result<String, JsValue> {
    Obfuscator::builder().seed(seed).build().obfuscate(html).map_err(to_js)
}

/// Obfuscate with the headline advanced layers enabled (honeypots +
/// structural obfuscation + AST mangling/string-array/dead-code).
///
/// `seed < 0` selects non-deterministic output.
#[wasm_bindgen]
pub fn obfuscate_max(html: &str, honeypots: usize, seed: i64) -> Result<String, JsValue> {
    let mut b = Obfuscator::builder()
        .inject_honeypots(honeypots > 0)
        .honeypot_count(honeypots)
        .structural_obfuscation(true)
        .js_ast(true)
        .mangle_identifiers(true)
        .js_string_encoding(JsStringEncoding::Array)
        .dead_code_injection(true);
    if seed >= 0 {
        b = b.seed(seed as u64);
    }
    b.build().obfuscate(html).map_err(to_js)
}

fn to_js(e: crate::SsukkaError) -> JsValue {
    JsValue::from_str(&e.to_string())
}
