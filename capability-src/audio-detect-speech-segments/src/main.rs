//! Thin WASI-command shim for `audio.detect-speech-segments`. All engine
//! logic (model parsing, the STFT/conv/LSTM forward pass, JSON wiring) lives
//! in `lib.rs`, which `audio.transcribe-speech` (registry#477) also depends
//! on directly to reuse the same Silero VAD scoring as a pre-check before
//! its own, much more expensive Whisper forward pass.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    #[cfg(feature = "full-model")]
    {
        wasi_capability_runtime::run_capability(audio_detect_speech_segments_agent::run);
    }
    #[cfg(not(feature = "full-model"))]
    {
        // Release builds must enable `full-model`.
        loop {
            core::arch::wasm32::unreachable()
        }
    }
}
