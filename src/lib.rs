pub mod analysis;
pub mod config;
pub mod css;
pub mod error;
pub mod html;
pub mod js;
pub mod obfuscator;
pub mod symbol_map;
pub mod transform;

pub use config::ObfuscationConfig;
pub use error::{Result, SsukkaError};
pub use obfuscator::{Obfuscator, ObfuscatorBuilder};

/// Obfuscate HTML with default settings.
///
/// This is a convenience function equivalent to:
/// ```
/// # let html = "<div>hello</div>";
/// ssukka::Obfuscator::default().obfuscate(html);
/// ```
pub fn obfuscate(html: &str) -> Result<String> {
    Obfuscator::default().obfuscate(html)
}
