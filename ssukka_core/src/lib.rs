pub mod ai_opt_out;
pub mod analysis;
pub mod config;
pub mod css;
pub mod error;
pub mod honeypot;
pub mod html;
pub mod inline;
pub mod js;
pub mod js_ast;
pub mod obfuscator;
pub mod structural;
pub mod symbol_map;
pub mod transform;
pub mod watermark;
pub mod word_split;
pub mod wordlist;

pub use config::ObfuscationConfig;
pub use error::{Result, SsukkaError};
pub use obfuscator::{Obfuscator, ObfuscatorBuilder};

/// Obfuscate HTML with default settings. Equivalent to:
/// ```
/// # let html = "<div>hello</div>";
/// ssukka_core::Obfuscator::default().obfuscate(html);
/// ```
pub fn obfuscate(html: &str) -> Result<String> {
    Obfuscator::default().obfuscate(html)
}
