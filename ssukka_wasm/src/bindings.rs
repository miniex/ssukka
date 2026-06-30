//! wasm-bindgen exports (compiled only for the `wasm32` target).

use ssukka_core::ai_opt_out;
use ssukka_core::config::JsStringEncoding;
use ssukka_core::Obfuscator;
use wasm_bindgen::prelude::*;

/// Obfuscate HTML with default settings (non-deterministic output).
#[wasm_bindgen]
pub fn obfuscate(html: &str) -> Result<String, JsValue> {
    ssukka_core::obfuscate(html).map_err(to_js)
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

/// AIPREF `Content-Usage` response-header value to stamp at the edge.
#[wasm_bindgen]
pub fn content_usage_header() -> String {
    ai_opt_out::content_usage_header().to_string()
}

/// A ready-to-serve `robots.txt` (AIPREF opt-out + a Disallow per AI crawler).
#[wasm_bindgen]
pub fn robots_txt() -> String {
    ai_opt_out::robots_txt()
}

/// The `/.well-known/tdmrep.json` body; pass a policy URL, or "" / null for none.
#[wasm_bindgen]
pub fn well_known_tdmrep_json(tdm_policy: Option<String>) -> String {
    ai_opt_out::well_known_tdmrep_json(tdm_policy.as_deref().filter(|s| !s.is_empty()))
}

fn to_js(e: ssukka_core::SsukkaError) -> JsValue {
    JsValue::from_str(&e.to_string())
}
