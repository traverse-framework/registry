//! Thin WASI-command shim for `text.detect-entities`. All engine logic
//! (model parsing, WordPiece tokenization, the BERT-encoder forward pass,
//! BIO decoding, JSON wiring) lives in `lib.rs`, which `text.redact-entities`
//! (registry#481) also depends on directly to reuse the same DistilBERT-NER
//! forward pass rather than duplicating it.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    #[cfg(feature = "full-model")]
    {
        wasi_capability_runtime::run_capability(text_detect_entities_agent::run);
    }
    #[cfg(not(feature = "full-model"))]
    {
        // Release builds must enable `full-model`.
        loop {
            core::arch::wasm32::unreachable()
        }
    }
}
