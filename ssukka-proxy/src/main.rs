//! ssukka-proxy - serve-time HTML obfuscation reverse proxy (skeleton).
//!
//! Planned: obfuscate responses through [`ssukka_core`] (offline; the proxy is
//! the networked host) and emit the opt-out signals - per-request obfuscation
//! without touching the origin.

fn main() {
    eprintln!("ssukka-proxy is not implemented yet - see TODOS.");
    eprintln!("For now obfuscate with the `ssukka` CLI or embed `ssukka_core` in your server.");
    std::process::exit(1);
}
