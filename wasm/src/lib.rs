//! WebAssembly bindings for ssukka.
//!
//! ```sh
//! wasm-pack build wasm
//! # or: cargo build -p ssukka-wasm --release --target wasm32-unknown-unknown
//! ```
//!
//! Empty off the `wasm32` target, so a host build never pulls the wasm toolchain.
#[cfg(target_arch = "wasm32")]
mod bindings;
