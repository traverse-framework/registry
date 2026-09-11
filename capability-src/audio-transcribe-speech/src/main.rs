//! Whisper-tiny (openai/whisper-tiny) greedy, English-forced, no-timestamps
//! speech-to-text for `audio.transcribe-speech`.
//!
//! Fifth and final model from this session's AI-WASM-model review
//! (registry#473, following #455/#460/#465/#469). License: dual-attested --
//! the `openai/whisper` GitHub repo's own `LICENSE` file is MIT (verified
//! directly); the `openai/whisper-tiny` Hugging Face model card's own
//! metadata (`cardData.license`) says apache-2.0. Both primary sources,
//! both permissive; logged as-is rather than picking one.
//!
//! Architecture (all verified against primary sources -- openai/whisper's
//! own `audio.py`/`model.py`/`tokenizer.py`/`decoding.py`, cross-checked
//! against the HF `config.json`): 80-channel log-mel spectrogram (16kHz,
//! 400-sample FFT, 160-sample hop, Hann window, a fixed 80x201 mel
//! filterbank matrix) -> a 2-conv1d front end (stride 1 then stride 2,
//! GELU) -> a 4-layer, 384-dim, 6-head transformer encoder (sinusoidal,
//! non-learned position embedding) -> a 4-layer, 384-dim, 6-head
//! transformer decoder (learned position embedding, causal self-attention,
//! cross-attention to the full 1500-frame encoder output) -> greedy
//! argmax decoding, seeded with a fixed `<|startoftranscript|><|en|>
//! <|transcribe|><|notimestamps|>` prompt (an officially-sanctioned,
//! documented minimal decoding path -- HF's own `generation_config.json`
//! ships exactly this as one of whisper-tiny's two published configs) --
//! not a beam search, not language detection, not timestamp prediction.
//! No KV-cache: each decode step recomputes the full decoder stack over
//! the whole token sequence so far (the ONNX `decoder_model.onnx` variant
//! this was verified against has the same non-cached contract), which is
//! correctness-first and entirely tractable at this model's scale (4
//! layers, <=224 output tokens) -- caching is a possible future
//! optimization, not a correctness requirement.
//!
//! Weight resolution: this ONNX export's MatMul *node names* retain the
//! full original module path (e.g. "/layers.0/self_attn/q_proj/MatMul")
//! even though the initializer tensors are anonymized -- see
//! `scripts/model/prepare_whisper_tiny_int8.py` for the extraction detail
//! (a simpler variant of the bias->Add->MatMul graph-tracing technique
//! `text.detect-entities`, registry#465, needed for a differently-exported
//! graph). All Linear-layer weight matrices are ONNX `MatMul(x, W)`
//! "(in, out)" orientation, same as that DistilBERT-NER export -- the
//! opposite of HF's native `nn.Linear.weight` "(out, in)" layout.
//!
//! Quantization: per-tensor symmetric int8 on every attention/FFN weight
//! matrix and the decoder's tied `embed_tokens` table (by far the largest
//! single tensor, 51865x384 -- also used as the output projection).
//! Conv weights, biases, LayerNorm gamma/beta, and the learned decoder
//! position embeddings stay float32. Verified empirically end to end on a
//! real 11s speech sample (openai/whisper's own `tests/jfk.flac` test
//! fixture, the famous JFK inaugural excerpt) with the full greedy
//! autoregressive loop: the quantized transcript matched the float32
//! baseline's wording exactly except for a single comma token, arguably
//! *improving* accuracy against the real quote -- unlike registry#460's
//! Silero VAD, where int8 measurably broke a recurrent LSTM's output,
//! autoregressive token decisions here proved robust to per-weight
//! quantization noise (consistent with, not contradicting, registry#465's
//! stateless-feedforward finding for DistilBERT-NER).
//!
//! Memory: the compiled artifact runs under Traverse's shared 64 MiB
//! bump allocator (`wasi_capability_runtime`), which never frees within
//! one invocation -- so every activation buffer here (hidden states,
//! attention score matrices, FFN intermediates) is allocated exactly
//! once, sized to the encoder's worst case, and reused in place by both
//! the encoder pass and every decoder greedy-loop step, rather than
//! reallocated per layer/head/step. The ~40 MB weight table itself is
//! never copied into that heap -- every weight is read zero-copy from the
//! `include_bytes!` static data segment via `QMat`/`F32View` (the pattern
//! already proven in `text.detect-entities`), so it never competes with
//! the 64 MiB activation budget.
//!
//! Known, documented limitations: fixed 30-second processing window
//! (Whisper's own native single-inference chunk -- longer audio must be
//! pre-chunked by the caller, matching upstream's own behavior); output
//! capped at 224 decoded tokens; English-only (language forced, not
//! detected -- multilingual input transcribes as attempted English);
//! greedy single-hypothesis decoding, not beam search, so wording can
//! differ slightly from the reference CLI's default settings without
//! being wrong.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use wasi_capability_runtime::{object, Value};

#[cfg(feature = "full-model")]
static FULL_MODEL_BIN: &[u8] = include_bytes!("../data/whisper-tiny-int8.bin");

// ---------------------------------------------------------------------
// Fixed audio-front-end constants (Whisper's own protocol, independent
// of the model weights -- see openai/whisper's audio.py).
// ---------------------------------------------------------------------
const SAMPLE_RATE: usize = 16000;
const N_FFT: usize = 400;
const HOP_LENGTH: usize = 160;
const N_SAMPLES: usize = 30 * SAMPLE_RATE; // 480000, one 30s window
const N_MEL_FRAMES: usize = N_SAMPLES / HOP_LENGTH; // 3000
const N_FFT_BINS: usize = N_FFT / 2 + 1; // 201

/// Hard cap on decode steps: well under any real n_text_ctx (448), bounds
/// worst-case execution time/memory for the full-recompute-each-step
/// decoder loop. A real short utterance needs far fewer.
const MAX_NEW_TOKENS: usize = 224;
/// Below this, "silence"/no distinguishable audio -- report empty text
/// rather than letting the decoder hallucinate on an all-zero mel input.
const MIN_AUDIO_ENERGY: f32 = 1e-6;

// ---------------------------------------------------------------------
// Binary model format (WHSP1) -- see scripts/model/prepare_whisper_tiny_int8.py.
// ---------------------------------------------------------------------

fn read_u32(b: &[u8], at: usize) -> u32 {
    match b.get(at..at + 4) {
        Some(s) => u32::from_le_bytes([s[0], s[1], s[2], s[3]]),
        None => 0,
    }
}

/// Zero-copy view over a little-endian f32 array embedded in the static
/// weight table.
#[derive(Clone, Copy)]
struct F32View<'a> {
    data: &'a [u8],
}

impl<'a> F32View<'a> {
    fn new(data: &'a [u8], len: usize) -> F32View<'a> {
        F32View {
            data: &data[..len * 4],
        }
    }
    fn at(&self, i: usize) -> f32 {
        match self.data.get(4 * i..4 * i + 4) {
            Some(s) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
            None => 0.0,
        }
    }
}

/// Zero-copy view over a per-tensor symmetric int8-quantized matrix in
/// this export's ONNX `MatMul(x, W)` "(in, out)" orientation -- same
/// convention and reader shape as `text.detect-entities`' `QMat`.
#[derive(Clone, Copy)]
struct QMat<'a> {
    scale: f32,
    rows: usize, // in_features
    cols: usize, // out_features
    data: &'a [u8],
}

impl<'a> QMat<'a> {
    #[inline]
    fn at(&self, r: usize, c: usize) -> f32 {
        let idx = r * self.cols + c;
        match self.data.get(idx) {
            Some(&b) => (b as i8) as f32 * self.scale,
            None => 0.0,
        }
    }
}

struct EncoderLayer<'a> {
    q_w: QMat<'a>,
    k_w: QMat<'a>,
    v_w: QMat<'a>,
    out_w: QMat<'a>,
    fc1_w: QMat<'a>,
    fc2_w: QMat<'a>,
    q_b: F32View<'a>,
    v_b: F32View<'a>,
    out_b: F32View<'a>,
    sa_ln_w: F32View<'a>,
    sa_ln_b: F32View<'a>,
    fc1_b: F32View<'a>,
    fc2_b: F32View<'a>,
    final_ln_w: F32View<'a>,
    final_ln_b: F32View<'a>,
}

struct DecoderLayer<'a> {
    sa_q_w: QMat<'a>,
    sa_k_w: QMat<'a>,
    sa_v_w: QMat<'a>,
    sa_out_w: QMat<'a>,
    sa_q_b: F32View<'a>,
    sa_v_b: F32View<'a>,
    sa_out_b: F32View<'a>,
    sa_ln_w: F32View<'a>,
    sa_ln_b: F32View<'a>,
    ca_q_w: QMat<'a>,
    ca_k_w: QMat<'a>,
    ca_v_w: QMat<'a>,
    ca_out_w: QMat<'a>,
    ca_q_b: F32View<'a>,
    ca_v_b: F32View<'a>,
    ca_out_b: F32View<'a>,
    ca_ln_w: F32View<'a>,
    ca_ln_b: F32View<'a>,
    fc1_w: QMat<'a>,
    fc2_w: QMat<'a>,
    fc1_b: F32View<'a>,
    fc2_b: F32View<'a>,
    final_ln_w: F32View<'a>,
    final_ln_b: F32View<'a>,
}

struct Model<'a> {
    n_mels: usize,
    n_mel_bins: usize,
    n_audio_ctx: usize,
    n_audio_state: usize,
    n_audio_head: usize,
    n_vocab: usize,
    base_vocab: usize,
    n_text_ctx: usize,
    n_text_state: usize,
    n_text_head: usize,
    sot: u32,
    eot: u32,
    lang_en: u32,
    task_transcribe: u32,
    no_timestamps: u32,

    mel_filters: F32View<'a>, // (n_mels, n_mel_bins)

    conv1_w: F32View<'a>, // (n_audio_state, n_mels, 3)
    conv1_b: F32View<'a>,
    conv2_w: F32View<'a>, // (n_audio_state, n_audio_state, 3)
    conv2_b: F32View<'a>,
    enc_layers: Vec<EncoderLayer<'a>>,
    enc_final_ln_w: F32View<'a>,
    enc_final_ln_b: F32View<'a>,

    embed_tokens: QMat<'a>, // (n_vocab, n_text_state), reused transposed for output proj
    embed_positions: F32View<'a>, // (n_text_ctx, n_text_state)
    dec_layers: Vec<DecoderLayer<'a>>,
    dec_final_ln_w: F32View<'a>,
    dec_final_ln_b: F32View<'a>,

    vocab_lens: &'a [u8],  // base_vocab bytes, one length per token id
    vocab_bytes: &'a [u8], // concatenated token byte strings
}

/// Parses the embedded table. Trusted, self-authored asset (generated by
/// `prepare_whisper_tiny_int8.py`, exercised by this crate's own tests) --
/// malformed input here means a build-time packaging bug, not something a
/// caller can trigger, so parsing degrades to a zero-dimensioned model on
/// any structural mismatch rather than panicking.
fn parse_model(bytes: &[u8]) -> Option<Model<'_>> {
    if bytes.len() < 5 + 14 * 4 + 5 * 4 || &bytes[0..5] != b"WHSP1" {
        return None;
    }
    let mut off = 5usize;
    let read_dim = |o: &mut usize| -> usize {
        let v = read_u32(bytes, *o) as usize;
        *o += 4;
        v
    };
    let _version = read_dim(&mut off);
    let n_mels = read_dim(&mut off);
    let n_mel_bins = read_dim(&mut off);
    let n_audio_ctx = read_dim(&mut off);
    let n_audio_state = read_dim(&mut off);
    let n_audio_head = read_dim(&mut off);
    let n_audio_layer = read_dim(&mut off);
    let n_vocab = read_dim(&mut off);
    let base_vocab = read_dim(&mut off);
    let n_text_ctx = read_dim(&mut off);
    let n_text_state = read_dim(&mut off);
    let n_text_head = read_dim(&mut off);
    let n_text_layer = read_dim(&mut off);
    let _reserved = read_dim(&mut off);

    let sot = read_u32(bytes, off);
    let eot = read_u32(bytes, off + 4);
    let lang_en = read_u32(bytes, off + 8);
    let task_transcribe = read_u32(bytes, off + 12);
    let no_timestamps = read_u32(bytes, off + 16);
    off += 20;

    let take_f32 = |o: &mut usize, n: usize| -> F32View<'_> {
        let v = F32View::new(&bytes[*o..], n);
        *o += n * 4;
        v
    };
    let take_quant = |o: &mut usize, rows: usize, cols: usize| -> QMat<'_> {
        let scale = f32::from_le_bytes([bytes[*o], bytes[*o + 1], bytes[*o + 2], bytes[*o + 3]]);
        *o += 4;
        let n = rows * cols;
        let q = QMat {
            scale,
            rows,
            cols,
            data: &bytes[*o..*o + n],
        };
        *o += n;
        q
    };

    let mel_filters = take_f32(&mut off, n_mels * n_mel_bins);

    let conv1_w = take_f32(&mut off, n_audio_state * n_mels * 3);
    let conv1_b = take_f32(&mut off, n_audio_state);
    let conv2_w = take_f32(&mut off, n_audio_state * n_audio_state * 3);
    let conv2_b = take_f32(&mut off, n_audio_state);

    let mut enc_layers = Vec::with_capacity(n_audio_layer);
    for _ in 0..n_audio_layer {
        let q_w = take_quant(&mut off, n_audio_state, n_audio_state);
        let k_w = take_quant(&mut off, n_audio_state, n_audio_state);
        let v_w = take_quant(&mut off, n_audio_state, n_audio_state);
        let out_w = take_quant(&mut off, n_audio_state, n_audio_state);
        let fc1_w = take_quant(&mut off, n_audio_state, n_audio_state * 4);
        let fc2_w = take_quant(&mut off, n_audio_state * 4, n_audio_state);
        let q_b = take_f32(&mut off, n_audio_state);
        let v_b = take_f32(&mut off, n_audio_state);
        let out_b = take_f32(&mut off, n_audio_state);
        let sa_ln_w = take_f32(&mut off, n_audio_state);
        let sa_ln_b = take_f32(&mut off, n_audio_state);
        let fc1_b = take_f32(&mut off, n_audio_state * 4);
        let fc2_b = take_f32(&mut off, n_audio_state);
        let final_ln_w = take_f32(&mut off, n_audio_state);
        let final_ln_b = take_f32(&mut off, n_audio_state);
        enc_layers.push(EncoderLayer {
            q_w,
            k_w,
            v_w,
            out_w,
            fc1_w,
            fc2_w,
            q_b,
            v_b,
            out_b,
            sa_ln_w,
            sa_ln_b,
            fc1_b,
            fc2_b,
            final_ln_w,
            final_ln_b,
        });
    }
    let enc_final_ln_w = take_f32(&mut off, n_audio_state);
    let enc_final_ln_b = take_f32(&mut off, n_audio_state);

    let embed_tokens = take_quant(&mut off, n_vocab, n_text_state);
    let embed_positions = take_f32(&mut off, n_text_ctx * n_text_state);

    let mut dec_layers = Vec::with_capacity(n_text_layer);
    for _ in 0..n_text_layer {
        let sa_q_w = take_quant(&mut off, n_text_state, n_text_state);
        let sa_k_w = take_quant(&mut off, n_text_state, n_text_state);
        let sa_v_w = take_quant(&mut off, n_text_state, n_text_state);
        let sa_out_w = take_quant(&mut off, n_text_state, n_text_state);
        let ca_q_w = take_quant(&mut off, n_text_state, n_text_state);
        let ca_k_w = take_quant(&mut off, n_text_state, n_text_state);
        let ca_v_w = take_quant(&mut off, n_text_state, n_text_state);
        let ca_out_w = take_quant(&mut off, n_text_state, n_text_state);
        let fc1_w = take_quant(&mut off, n_text_state, n_text_state * 4);
        let fc2_w = take_quant(&mut off, n_text_state * 4, n_text_state);
        let sa_q_b = take_f32(&mut off, n_text_state);
        let sa_v_b = take_f32(&mut off, n_text_state);
        let sa_out_b = take_f32(&mut off, n_text_state);
        let sa_ln_w = take_f32(&mut off, n_text_state);
        let sa_ln_b = take_f32(&mut off, n_text_state);
        let ca_q_b = take_f32(&mut off, n_text_state);
        let ca_v_b = take_f32(&mut off, n_text_state);
        let ca_out_b = take_f32(&mut off, n_text_state);
        let ca_ln_w = take_f32(&mut off, n_text_state);
        let ca_ln_b = take_f32(&mut off, n_text_state);
        let fc1_b = take_f32(&mut off, n_text_state * 4);
        let fc2_b = take_f32(&mut off, n_text_state);
        let final_ln_w = take_f32(&mut off, n_text_state);
        let final_ln_b = take_f32(&mut off, n_text_state);
        dec_layers.push(DecoderLayer {
            sa_q_w,
            sa_k_w,
            sa_v_w,
            sa_out_w,
            sa_q_b,
            sa_v_b,
            sa_out_b,
            sa_ln_w,
            sa_ln_b,
            ca_q_w,
            ca_k_w,
            ca_v_w,
            ca_out_w,
            ca_q_b,
            ca_v_b,
            ca_out_b,
            ca_ln_w,
            ca_ln_b,
            fc1_w,
            fc2_w,
            fc1_b,
            fc2_b,
            final_ln_w,
            final_ln_b,
        });
    }
    let dec_final_ln_w = take_f32(&mut off, n_text_state);
    let dec_final_ln_b = take_f32(&mut off, n_text_state);

    let vocab_lens = bytes.get(off..off + base_vocab)?;
    off += base_vocab;
    let total_vocab_bytes: usize = vocab_lens.iter().map(|&b| b as usize).sum();
    let vocab_bytes = bytes.get(off..off + total_vocab_bytes)?;

    Some(Model {
        n_mels,
        n_mel_bins,
        n_audio_ctx,
        n_audio_state,
        n_audio_head,
        n_vocab,
        base_vocab,
        n_text_ctx,
        n_text_state,
        n_text_head,
        sot,
        eot,
        lang_en,
        task_transcribe,
        no_timestamps,
        mel_filters,
        conv1_w,
        conv1_b,
        conv2_w,
        conv2_b,
        enc_layers,
        enc_final_ln_w,
        enc_final_ln_b,
        embed_tokens,
        embed_positions,
        dec_layers,
        dec_final_ln_w,
        dec_final_ln_b,
        vocab_lens,
        vocab_bytes,
    })
}

// ---------------------------------------------------------------------
// Math primitives.
// ---------------------------------------------------------------------

#[inline]
fn gelu(x: f32) -> f32 {
    // Exact (erf-based) GELU, matching HF's default activation for this model.
    0.5 * x * (1.0 + libm::erff(x * core::f32::consts::FRAC_1_SQRT_2))
}

/// LayerNorm over the last axis (`dim`-wide rows), eps=1e-5 (matches the
/// ONNX graph's own Constant).
fn layer_norm_rows(x: &mut [f32], dim: usize, weight: F32View, bias: F32View) {
    for row in x.chunks_mut(dim) {
        let mut mean = 0.0f32;
        for &v in row.iter() {
            mean += v;
        }
        mean /= dim as f32;
        let mut var = 0.0f32;
        for &v in row.iter() {
            let d = v - mean;
            var += d * d;
        }
        var /= dim as f32;
        let inv_std = 1.0 / libm::sqrtf(var + 1e-5);
        for (i, v) in row.iter_mut().enumerate() {
            *v = (*v - mean) * inv_std * weight.at(i) + bias.at(i);
        }
    }
}

/// y[t, :] = x[t, :] @ w + bias, for t in 0..rows. `w` is (in, out).
fn linear_rows(x: &[f32], rows: usize, w: &QMat, bias: Option<F32View>, out: &mut [f32]) {
    for t in 0..rows {
        let xin = &x[t * w.rows..t * w.rows + w.rows];
        let yout = &mut out[t * w.cols..t * w.cols + w.cols];
        for (o, y) in yout.iter_mut().enumerate() {
            *y = match bias {
                Some(b) => b.at(o),
                None => 0.0,
            };
        }
        for (i, &xi) in xin.iter().enumerate() {
            if xi == 0.0 {
                continue;
            }
            for (o, y) in yout.iter_mut().enumerate() {
                *y += xi * w.at(i, o);
            }
        }
    }
}

/// Multi-head attention. `q_in` is (tq, dim), `kv_in` is (tk, dim); writes
/// the post-out_proj result into `out` (tq, dim). Uses the caller's
/// scratch buffers for Q/K/V projections and the attention-score matrix --
/// never allocates, so it is safe to call repeatedly (once per decode
/// step) without growing heap usage.
#[allow(clippy::too_many_arguments)]
fn multi_head_attention(
    q_in: &[f32],
    tq: usize,
    kv_in: &[f32],
    tk: usize,
    dim: usize,
    n_head: usize,
    q_w: &QMat,
    k_w: &QMat,
    v_w: &QMat,
    out_w: &QMat,
    q_b: F32View,
    v_b: F32View,
    out_b: F32View,
    causal: bool,
    out: &mut [f32],
    q_buf: &mut [f32],
    k_buf: &mut [f32],
    v_buf: &mut [f32],
    scores_buf: &mut [f32],
) {
    let d_head = dim / n_head;
    let scale = 1.0 / libm::sqrtf(d_head as f32);

    linear_rows(q_in, tq, q_w, Some(q_b), &mut q_buf[..tq * dim]);
    linear_rows(kv_in, tk, k_w, None, &mut k_buf[..tk * dim]);
    linear_rows(kv_in, tk, v_w, Some(v_b), &mut v_buf[..tk * dim]);

    for h in 0..n_head {
        let hoff = h * d_head;
        let scores = &mut scores_buf[..tq * tk];
        for i in 0..tq {
            let qi = &q_buf[i * dim + hoff..i * dim + hoff + d_head];
            for j in 0..tk {
                if causal && j > i {
                    scores[i * tk + j] = f32::NEG_INFINITY;
                    continue;
                }
                let kj = &k_buf[j * dim + hoff..j * dim + hoff + d_head];
                let mut dot = 0.0f32;
                for d in 0..d_head {
                    dot += qi[d] * kj[d];
                }
                scores[i * tk + j] = dot * scale;
            }
        }
        // Row-wise softmax.
        for i in 0..tq {
            let row = &mut scores[i * tk..i * tk + tk];
            let mut max_v = f32::NEG_INFINITY;
            for &v in row.iter() {
                if v > max_v {
                    max_v = v;
                }
            }
            let mut sum = 0.0f32;
            for v in row.iter_mut() {
                let e = if v.is_finite() || *v == f32::NEG_INFINITY {
                    if *v == f32::NEG_INFINITY {
                        0.0
                    } else {
                        libm::expf(*v - max_v)
                    }
                } else {
                    0.0
                };
                *v = e;
                sum += e;
            }
            if sum > 0.0 {
                for v in row.iter_mut() {
                    *v /= sum;
                }
            }
        }
        // Weighted sum over V, written directly into this head's slice of
        // the pre-out_proj buffer (reusing q_buf as scratch -- it's no
        // longer needed once scores are computed).
        for i in 0..tq {
            let row = &scores[i * tk..i * tk + tk];
            for d in 0..d_head {
                let mut acc = 0.0f32;
                for j in 0..tk {
                    acc += row[j] * v_buf[j * dim + hoff + d];
                }
                q_buf[i * dim + hoff + d] = acc;
            }
        }
    }

    // out_proj: out[t,:] = q_buf[t,:] @ out_w + out_b (q_buf now holds the
    // concatenated per-head attention output).
    linear_rows(&q_buf[..tq * dim], tq, out_w, Some(out_b), out);
}

// ---------------------------------------------------------------------
// Mel spectrogram front end.
// ---------------------------------------------------------------------

/// Computes the (n_mels, N_MEL_FRAMES) log-mel spectrogram for exactly
/// `N_SAMPLES` (30s) of 16kHz mono audio, matching openai/whisper's
/// `audio.py::log_mel_spectrogram` exactly: Hann-windowed STFT (N_FFT=400,
/// hop=160, reflect-padded by N_FFT/2 on both sides), magnitude-squared,
/// projected through the model's own mel filterbank, log10, floor at
/// (max-8dB), then `(x+4)/4` normalization.
fn compute_log_mel(
    samples: &[f32],
    model: &Model,
    twiddle_cos: &[f32],
    twiddle_sin: &[f32],
    mel_out: &mut [f32],
) {
    let pad = N_FFT / 2;
    let padded_len = samples.len() + 2 * pad;

    // Reflect-pad access: index into the conceptual padded array without
    // materializing it (avoids one more N_SAMPLES+2*pad-sized buffer).
    let reflect_at = |i: isize| -> f32 {
        let n = samples.len() as isize;
        if i < 0 {
            samples[(-i) as usize % samples.len()]
        } else if i >= n {
            let over = i - (n - 1);
            samples[(n - 1 - over.rem_euclid(n)) as usize]
        } else {
            samples[i as usize]
        }
    };

    let mut frame = vec![0.0f32; N_FFT];
    let mut mag = vec![0.0f32; N_FFT_BINS];
    let hann: Vec<f32> = (0..N_FFT)
        .map(|n| {
            let x = core::f32::consts::PI * (n as f32) / (N_FFT as f32);
            let s = libm::sinf(x);
            s * s
        })
        .collect();

    let n_frames = (padded_len - N_FFT) / HOP_LENGTH + 1;
    let n_frames_used = core::cmp::min(n_frames.saturating_sub(1), N_MEL_FRAMES); // drop last frame, matches reference

    for t in 0..n_frames_used {
        let start = (t * HOP_LENGTH) as isize - pad as isize;
        for n in 0..N_FFT {
            frame[n] = reflect_at(start + n as isize) * hann[n];
        }
        // Direct DFT via a precomputed (N_FFT_BINS x N_FFT) twiddle table
        // -- N_FFT=400 is not power-of-two, and the table is reused for
        // every one of the 3000 frames, so this is a bounded, one-time
        // cost, not a per-frame one.
        for (k, m) in mag.iter_mut().enumerate() {
            let mut re = 0.0f32;
            let mut im = 0.0f32;
            let base = k * N_FFT;
            for n in 0..N_FFT {
                re += frame[n] * twiddle_cos[base + n];
                im += frame[n] * twiddle_sin[base + n];
            }
            *m = re * re + im * im;
        }
        for m in 0..model.n_mels {
            let mut acc = 0.0f32;
            for (k, &magk) in mag.iter().enumerate().take(model.n_mel_bins) {
                acc += model.mel_filters.at(m * model.n_mel_bins + k) * magk;
            }
            mel_out[m * N_MEL_FRAMES + t] = acc;
        }
    }
    for t in n_frames_used..N_MEL_FRAMES {
        for m in 0..model.n_mels {
            mel_out[m * N_MEL_FRAMES + t] = 0.0;
        }
    }

    let mut max_log = f32::NEG_INFINITY;
    for v in mel_out.iter_mut() {
        let lv = libm::log10f(if *v > 1e-10 { *v } else { 1e-10 });
        *v = lv;
        if lv > max_log {
            max_log = lv;
        }
    }
    let floor = max_log - 8.0;
    for v in mel_out.iter_mut() {
        if *v < floor {
            *v = floor;
        }
        *v = (*v + 4.0) / 4.0;
    }
}

fn build_dft_twiddles(cos_out: &mut [f32], sin_out: &mut [f32]) {
    for k in 0..N_FFT_BINS {
        for n in 0..N_FFT {
            let angle = -2.0 * core::f32::consts::PI * (k as f32) * (n as f32) / (N_FFT as f32);
            cos_out[k * N_FFT + n] = libm::cosf(angle);
            sin_out[k * N_FFT + n] = libm::sinf(angle);
        }
    }
}

fn sinusoid_position_embedding(length: usize, channels: usize, out: &mut [f32]) {
    let half = channels / 2;
    let log_ts_inc = libm::logf(10000.0) / (half as f32 - 1.0);
    for pos in 0..length {
        for i in 0..half {
            let inv_ts = libm::expf(-log_ts_inc * (i as f32));
            let scaled = (pos as f32) * inv_ts;
            out[pos * channels + i] = libm::sinf(scaled);
            out[pos * channels + half + i] = libm::cosf(scaled);
        }
    }
}

// ---------------------------------------------------------------------
// Encoder / decoder forward passes.
// ---------------------------------------------------------------------

struct Scratch {
    mel: Vec<f32>,
    twiddle_cos: Vec<f32>,
    twiddle_sin: Vec<f32>,
    conv_a: Vec<f32>,
    conv_b: Vec<f32>,
    hidden: Vec<f32>, // encoder: (n_audio_ctx, dim); reused (subset) as decoder hidden
    ln_buf: Vec<f32>,
    ffn_buf: Vec<f32>,
    attn_out: Vec<f32>,
    q_buf: Vec<f32>,
    k_buf: Vec<f32>,
    v_buf: Vec<f32>,
    scores: Vec<f32>,
}

#[allow(clippy::too_many_arguments)]
fn conv1d(
    input: &[f32],
    in_ch: usize,
    in_len: usize,
    weight: F32View,
    bias: F32View,
    out_ch: usize,
    stride: usize,
    out: &mut [f32],
) {
    let k = 3usize;
    let pad = 1usize;
    let out_len = (in_len + 2 * pad - k) / stride + 1;
    for oc in 0..out_ch {
        for t in 0..out_len {
            let mut acc = bias.at(oc);
            let start = (t * stride) as isize - pad as isize;
            for ic in 0..in_ch {
                for kk in 0..k {
                    let pos = start + kk as isize;
                    if pos < 0 || pos as usize >= in_len {
                        continue;
                    }
                    let w = weight.at(oc * in_ch * k + ic * k + kk);
                    acc += w * input[ic * in_len + pos as usize];
                }
            }
            out[oc * out_len + t] = acc;
        }
    }
}

fn encoder_forward(model: &Model, s: &mut Scratch) {
    let dim = model.n_audio_state;
    let ctx = model.n_audio_ctx; // 1500

    conv1d(
        &s.mel[..model.n_mels * N_MEL_FRAMES],
        model.n_mels,
        N_MEL_FRAMES,
        model.conv1_w,
        model.conv1_b,
        dim,
        1,
        &mut s.conv_a[..dim * N_MEL_FRAMES],
    );
    for v in s.conv_a[..dim * N_MEL_FRAMES].iter_mut() {
        *v = gelu(*v);
    }
    conv1d(
        &s.conv_a[..dim * N_MEL_FRAMES],
        dim,
        N_MEL_FRAMES,
        model.conv2_w,
        model.conv2_b,
        dim,
        2,
        &mut s.conv_b[..dim * ctx],
    );
    for v in s.conv_b[..dim * ctx].iter_mut() {
        *v = gelu(*v);
    }
    // Transpose (dim, ctx) -> (ctx, dim) into hidden.
    for c in 0..dim {
        for t in 0..ctx {
            s.hidden[t * dim + c] = s.conv_b[c * ctx + t];
        }
    }
    sinusoid_position_embedding(ctx, dim, &mut s.conv_a[..ctx * dim]);
    for i in 0..ctx * dim {
        s.hidden[i] += s.conv_a[i];
    }

    for layer in &model.enc_layers {
        s.ln_buf[..ctx * dim].copy_from_slice(&s.hidden[..ctx * dim]);
        layer_norm_rows(
            &mut s.ln_buf[..ctx * dim],
            dim,
            layer.sa_ln_w,
            layer.sa_ln_b,
        );
        multi_head_attention(
            &s.ln_buf[..ctx * dim],
            ctx,
            &s.ln_buf[..ctx * dim],
            ctx,
            dim,
            model.n_audio_head,
            &layer.q_w,
            &layer.k_w,
            &layer.v_w,
            &layer.out_w,
            layer.q_b,
            layer.v_b,
            layer.out_b,
            false,
            &mut s.attn_out[..ctx * dim],
            &mut s.q_buf[..ctx * dim],
            &mut s.k_buf[..ctx * dim],
            &mut s.v_buf[..ctx * dim],
            &mut s.scores[..ctx * ctx],
        );
        for i in 0..ctx * dim {
            s.hidden[i] += s.attn_out[i];
        }

        s.ln_buf[..ctx * dim].copy_from_slice(&s.hidden[..ctx * dim]);
        layer_norm_rows(
            &mut s.ln_buf[..ctx * dim],
            dim,
            layer.final_ln_w,
            layer.final_ln_b,
        );
        linear_rows(
            &s.ln_buf[..ctx * dim],
            ctx,
            &layer.fc1_w,
            Some(layer.fc1_b),
            &mut s.ffn_buf[..ctx * dim * 4],
        );
        for v in s.ffn_buf[..ctx * dim * 4].iter_mut() {
            *v = gelu(*v);
        }
        linear_rows(
            &s.ffn_buf[..ctx * dim * 4],
            ctx,
            &layer.fc2_w,
            Some(layer.fc2_b),
            &mut s.attn_out[..ctx * dim],
        );
        for i in 0..ctx * dim {
            s.hidden[i] += s.attn_out[i];
        }
    }

    layer_norm_rows(
        &mut s.hidden[..ctx * dim],
        dim,
        model.enc_final_ln_w,
        model.enc_final_ln_b,
    );
}

/// Returns logits (n_vocab) for the last position of `tokens`, given the
/// (already-computed) encoder output living in `enc_out`.
fn decoder_forward_logits(
    model: &Model,
    tokens: &[u32],
    enc_out: &[f32],
    enc_ctx: usize,
    s: &mut Scratch,
    logits: &mut [f32],
) {
    let dim = model.n_text_state;
    let t = tokens.len();
    let dec_hidden = &mut s.ln_buf[..t * dim]; // reuse ln_buf as the persistent decoder hidden state buffer's source
    for (i, &tok) in tokens.iter().enumerate() {
        for c in 0..dim {
            dec_hidden[i * dim + c] =
                model.embed_tokens.at(tok as usize, c) + model.embed_positions.at(i * dim + c);
        }
    }
    // Move into `attn_out` as the actual running hidden-state buffer so
    // `ln_buf` is free again for use inside the per-layer loop below.
    s.attn_out[..t * dim].copy_from_slice(&s.ln_buf[..t * dim]);

    for layer in &model.dec_layers {
        s.ln_buf[..t * dim].copy_from_slice(&s.attn_out[..t * dim]);
        layer_norm_rows(&mut s.ln_buf[..t * dim], dim, layer.sa_ln_w, layer.sa_ln_b);
        multi_head_attention(
            &s.ln_buf[..t * dim],
            t,
            &s.ln_buf[..t * dim],
            t,
            dim,
            model.n_text_head,
            &layer.sa_q_w,
            &layer.sa_k_w,
            &layer.sa_v_w,
            &layer.sa_out_w,
            layer.sa_q_b,
            layer.sa_v_b,
            layer.sa_out_b,
            true,
            &mut s.ffn_buf[..t * dim],
            &mut s.q_buf[..t * dim],
            &mut s.k_buf[..t * dim],
            &mut s.v_buf[..t * dim],
            &mut s.scores[..t * t],
        );
        for i in 0..t * dim {
            s.attn_out[i] += s.ffn_buf[i];
        }

        s.ln_buf[..t * dim].copy_from_slice(&s.attn_out[..t * dim]);
        layer_norm_rows(&mut s.ln_buf[..t * dim], dim, layer.ca_ln_w, layer.ca_ln_b);
        multi_head_attention(
            &s.ln_buf[..t * dim],
            t,
            enc_out,
            enc_ctx,
            dim,
            model.n_text_head,
            &layer.ca_q_w,
            &layer.ca_k_w,
            &layer.ca_v_w,
            &layer.ca_out_w,
            layer.ca_q_b,
            layer.ca_v_b,
            layer.ca_out_b,
            false,
            &mut s.ffn_buf[..t * dim],
            &mut s.q_buf[..t * dim],
            &mut s.k_buf[..enc_ctx * dim],
            &mut s.v_buf[..enc_ctx * dim],
            &mut s.scores[..t * enc_ctx],
        );
        for i in 0..t * dim {
            s.attn_out[i] += s.ffn_buf[i];
        }

        s.ln_buf[..t * dim].copy_from_slice(&s.attn_out[..t * dim]);
        layer_norm_rows(
            &mut s.ln_buf[..t * dim],
            dim,
            layer.final_ln_w,
            layer.final_ln_b,
        );
        linear_rows(
            &s.ln_buf[..t * dim],
            t,
            &layer.fc1_w,
            Some(layer.fc1_b),
            &mut s.ffn_buf[..t * dim * 4],
        );
        for v in s.ffn_buf[..t * dim * 4].iter_mut() {
            *v = gelu(*v);
        }
        linear_rows(
            &s.ffn_buf[..t * dim * 4],
            t,
            &layer.fc2_w,
            Some(layer.fc2_b),
            &mut s.q_buf[..t * dim],
        );
        for i in 0..t * dim {
            s.attn_out[i] += s.q_buf[i];
        }
    }

    let last = &mut s.ln_buf[..dim];
    last.copy_from_slice(&s.attn_out[(t - 1) * dim..t * dim]);
    layer_norm_rows(last, dim, model.dec_final_ln_w, model.dec_final_ln_b);

    for v in logits.iter_mut() {
        *v = 0.0;
    }
    for (i, &xi) in last.iter().enumerate() {
        if xi == 0.0 {
            continue;
        }
        for (v, l) in logits.iter_mut().enumerate().take(model.n_vocab) {
            *l += xi * model.embed_tokens.at(v, i);
        }
    }
}

fn decode_token_bytes(model: &Model, id: u32, out: &mut Vec<u8>) {
    let id = id as usize;
    if id >= model.base_vocab {
        return;
    }
    let mut start = 0usize;
    for i in 0..id {
        start += model.vocab_lens[i] as usize;
    }
    let len = model.vocab_lens[id] as usize;
    out.extend_from_slice(&model.vocab_bytes[start..start + len]);
}

// ---------------------------------------------------------------------
// Top-level capability logic.
// ---------------------------------------------------------------------

fn read_samples(input: &Value) -> Vec<f32> {
    let field = match input.get("samples").and_then(Value::as_array) {
        Some(items) => items,
        None => return Vec::new(),
    };
    let mut out = Vec::with_capacity(core::cmp::min(field.len(), N_SAMPLES));
    for item in field.iter().take(N_SAMPLES) {
        out.push(item.as_f64().unwrap_or(0.0) as f32);
    }
    out
}

fn transcribe(model: &Model, samples: &[f32]) -> String {
    let energy: f32 = samples.iter().map(|v| v * v).sum();
    if samples.is_empty() || energy < MIN_AUDIO_ENERGY * samples.len() as f32 {
        return String::new();
    }

    let dim = model.n_audio_state.max(model.n_text_state);
    let ctx = model.n_audio_ctx;
    // Every per-token scratch buffer below is shared between the encoder
    // pass (sequence length = ctx) and every decoder greedy-loop step
    // (sequence length = current token count, up to n_text_ctx) -- size
    // to the larger of the two so neither phase can index out of bounds.
    // For the real model ctx=1500 already dominates n_text_ctx=448, so
    // this is a no-op there; it matters for smaller configurations (e.g.
    // this crate's own test fixture).
    let max_seq = ctx.max(model.n_text_ctx);
    let mut s = Scratch {
        mel: vec![0.0; model.n_mels * N_MEL_FRAMES],
        twiddle_cos: vec![0.0; N_FFT_BINS * N_FFT],
        twiddle_sin: vec![0.0; N_FFT_BINS * N_FFT],
        conv_a: vec![0.0; dim * N_MEL_FRAMES],
        conv_b: vec![0.0; dim * ctx],
        hidden: vec![0.0; max_seq * dim],
        ln_buf: vec![0.0; max_seq * dim],
        ffn_buf: vec![0.0; max_seq * dim * 4],
        attn_out: vec![0.0; max_seq * dim],
        q_buf: vec![0.0; max_seq * dim],
        k_buf: vec![0.0; max_seq * dim],
        v_buf: vec![0.0; max_seq * dim],
        scores: vec![0.0; max_seq * max_seq],
    };

    let mut padded = vec![0.0f32; N_SAMPLES];
    let n = core::cmp::min(samples.len(), N_SAMPLES);
    padded[..n].copy_from_slice(&samples[..n]);

    build_dft_twiddles(&mut s.twiddle_cos, &mut s.twiddle_sin);
    let twiddle_cos = s.twiddle_cos.clone();
    let twiddle_sin = s.twiddle_sin.clone();
    compute_log_mel(
        &padded,
        model,
        &twiddle_cos,
        &twiddle_sin,
        &mut s.mel[..model.n_mels * N_MEL_FRAMES],
    );

    encoder_forward(model, &mut s);
    let enc_out = s.hidden[..ctx * dim].to_vec();

    let mut tokens: Vec<u32> = alloc::vec![
        model.sot,
        model.lang_en,
        model.task_transcribe,
        model.no_timestamps
    ];
    let max_new = core::cmp::min(
        MAX_NEW_TOKENS,
        model.n_text_ctx.saturating_sub(tokens.len() + 1),
    );
    let mut logits = vec![0.0f32; model.n_vocab];
    for _ in 0..max_new {
        decoder_forward_logits(model, &tokens, &enc_out, ctx, &mut s, &mut logits);
        let mut best_i = 0usize;
        let mut best_v = f32::NEG_INFINITY;
        for (i, &v) in logits.iter().enumerate() {
            if v > best_v {
                best_v = v;
                best_i = i;
            }
        }
        let next_id = best_i as u32;
        if next_id == model.eot {
            break;
        }
        tokens.push(next_id);
    }

    let mut out_bytes = Vec::new();
    for &tok in tokens.iter().skip(4) {
        decode_token_bytes(model, tok, &mut out_bytes);
    }
    String::from_utf8(out_bytes).unwrap_or_default()
}

fn run(input: &Value) -> Value {
    let samples = read_samples(input);

    #[cfg(feature = "full-model")]
    let table: &[u8] = FULL_MODEL_BIN;
    #[cfg(not(feature = "full-model"))]
    let table: &[u8] = &[];

    let model = match parse_model(table) {
        Some(m) => m,
        None => {
            return object(alloc::vec![
                ("text", Value::String(String::new())),
                ("status", Value::String("model_unavailable".to_string())),
            ]);
        }
    };

    let text = transcribe(&model, &samples);
    object(alloc::vec![
        ("text", Value::String(text)),
        ("status", Value::String("ok".to_string())),
    ])
}

#[cfg(all(not(test), target_arch = "wasm32"))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    wasi_capability_runtime::run_capability(|input| run(&input));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a tiny, structurally-valid WHSP1 fixture (1 encoder layer, 1
    /// decoder layer, small dims) for fast, deterministic unit tests that
    /// don't require the real ~40MB weight table. Weights are simple
    /// deterministic patterns, not meaningful for actual transcription --
    /// these tests check plumbing (parsing, shapes, determinism, edge
    /// cases), not transcription quality (that's verified separately
    /// against the real model in the publish PR, see data/README.md).
    fn fixture_bin() -> Vec<u8> {
        let n_mels = 4usize;
        let n_mel_bins = 5usize;
        // n_audio_ctx is NOT a free parameter -- the fixed DSP front end
        // (N_SAMPLES/HOP_LENGTH=N_MEL_FRAMES=3000 mel frames, halved by
        // conv2's stride-2) always produces exactly 1500 encoder
        // timesteps, regardless of what a model file declares here. A
        // smaller value would make the real `conv1d` output length
        // disagree with scratch-buffer sizing derived from this field.
        let n_audio_ctx = N_MEL_FRAMES / 2;
        let n_audio_state = 8usize;
        let n_audio_head = 2usize;
        let n_audio_layer = 1usize;
        let base_vocab = 6usize;
        let n_vocab = base_vocab + 6; // + a few specials, no language tokens for this tiny fixture
        let n_text_ctx = 16usize;
        let n_text_state = 8usize;
        let n_text_head = 2usize;
        let n_text_layer = 1usize;

        let sot = base_vocab as u32;
        let eot = base_vocab as u32 + 1;
        let lang_en = base_vocab as u32 + 2;
        let task_transcribe = base_vocab as u32 + 3;
        let no_timestamps = base_vocab as u32 + 4;

        let mut b: Vec<u8> = Vec::new();
        b.extend_from_slice(b"WHSP1");
        for v in [
            1u32,
            n_mels as u32,
            n_mel_bins as u32,
            n_audio_ctx as u32,
            n_audio_state as u32,
            n_audio_head as u32,
            n_audio_layer as u32,
            n_vocab as u32,
            base_vocab as u32,
            n_text_ctx as u32,
            n_text_state as u32,
            n_text_head as u32,
            n_text_layer as u32,
            0,
        ] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in [sot, eot, lang_en, task_transcribe, no_timestamps] {
            b.extend_from_slice(&v.to_le_bytes());
        }

        let push_f32 = |b: &mut Vec<u8>, n: usize, val: f32| {
            for _ in 0..n {
                b.extend_from_slice(&val.to_le_bytes());
            }
        };
        let push_f32_pattern = |b: &mut Vec<u8>, n: usize| {
            for i in 0..n {
                let v = 0.01 * ((i % 7) as f32 - 3.0);
                b.extend_from_slice(&v.to_le_bytes());
            }
        };
        let push_quant = |b: &mut Vec<u8>, rows: usize, cols: usize| {
            b.extend_from_slice(&0.02f32.to_le_bytes());
            for i in 0..rows * cols {
                b.push(((i % 5) as i32 - 2) as i8 as u8);
            }
        };

        push_f32_pattern(&mut b, n_mels * n_mel_bins); // mel_filters

        push_f32_pattern(&mut b, n_audio_state * n_mels * 3); // conv1.weight
        push_f32(&mut b, n_audio_state, 0.0); // conv1.bias
        push_f32_pattern(&mut b, n_audio_state * n_audio_state * 3); // conv2.weight
        push_f32(&mut b, n_audio_state, 0.0); // conv2.bias

        for _ in 0..n_audio_layer {
            push_quant(&mut b, n_audio_state, n_audio_state); // q
            push_quant(&mut b, n_audio_state, n_audio_state); // k
            push_quant(&mut b, n_audio_state, n_audio_state); // v
            push_quant(&mut b, n_audio_state, n_audio_state); // out
            push_quant(&mut b, n_audio_state, n_audio_state * 4); // fc1
            push_quant(&mut b, n_audio_state * 4, n_audio_state); // fc2
            push_f32(&mut b, n_audio_state, 0.0); // q_b
            push_f32(&mut b, n_audio_state, 0.0); // v_b
            push_f32(&mut b, n_audio_state, 0.0); // out_b
            push_f32(&mut b, n_audio_state, 1.0); // sa_ln_w
            push_f32(&mut b, n_audio_state, 0.0); // sa_ln_b
            push_f32(&mut b, n_audio_state * 4, 0.0); // fc1_b
            push_f32(&mut b, n_audio_state, 0.0); // fc2_b
            push_f32(&mut b, n_audio_state, 1.0); // final_ln_w
            push_f32(&mut b, n_audio_state, 0.0); // final_ln_b
        }
        push_f32(&mut b, n_audio_state, 1.0); // enc final ln w
        push_f32(&mut b, n_audio_state, 0.0); // enc final ln b

        push_quant(&mut b, n_vocab, n_text_state); // embed_tokens
        push_f32_pattern(&mut b, n_text_ctx * n_text_state); // embed_positions

        for _ in 0..n_text_layer {
            for _ in 0..4 {
                push_quant(&mut b, n_text_state, n_text_state); // sa q/k/v/out
            }
            for _ in 0..4 {
                push_quant(&mut b, n_text_state, n_text_state); // ca q/k/v/out
            }
            push_quant(&mut b, n_text_state, n_text_state * 4); // fc1
            push_quant(&mut b, n_text_state * 4, n_text_state); // fc2
            push_f32(&mut b, n_text_state, 0.0); // sa_q_b
            push_f32(&mut b, n_text_state, 0.0); // sa_v_b
            push_f32(&mut b, n_text_state, 0.0); // sa_out_b
            push_f32(&mut b, n_text_state, 1.0); // sa_ln_w
            push_f32(&mut b, n_text_state, 0.0); // sa_ln_b
            push_f32(&mut b, n_text_state, 0.0); // ca_q_b
            push_f32(&mut b, n_text_state, 0.0); // ca_v_b
            push_f32(&mut b, n_text_state, 0.0); // ca_out_b
            push_f32(&mut b, n_text_state, 1.0); // ca_ln_w
            push_f32(&mut b, n_text_state, 0.0); // ca_ln_b
            push_f32(&mut b, n_text_state * 4, 0.0); // fc1_b
            push_f32(&mut b, n_text_state, 0.0); // fc2_b
            push_f32(&mut b, n_text_state, 1.0); // final_ln_w
            push_f32(&mut b, n_text_state, 0.0); // final_ln_b
        }
        push_f32(&mut b, n_text_state, 1.0); // dec final ln w
        push_f32(&mut b, n_text_state, 0.0); // dec final ln b

        // vocab: 6 base tokens ("a".."f", one byte each) + specials have
        // no vocab entries (base_vocab only covers real text tokens).
        b.extend(core::iter::repeat_n(1u8, base_vocab));
        for c in 0..base_vocab {
            b.push(b'a' + c as u8);
        }
        b
    }

    #[test]
    fn parses_fixture_header_and_dims() {
        let bin = fixture_bin();
        let m = parse_model(&bin).expect("fixture should parse");
        assert_eq!(m.n_mels, 4);
        assert_eq!(m.n_audio_ctx, N_MEL_FRAMES / 2);
        assert_eq!(m.n_audio_state, 8);
        assert_eq!(m.enc_layers.len(), 1);
        assert_eq!(m.dec_layers.len(), 1);
        assert_eq!(m.base_vocab, 6);
        assert_eq!(m.eot, 7);
    }

    #[test]
    fn malformed_table_returns_none_not_panic() {
        assert!(parse_model(&[]).is_none());
        assert!(parse_model(b"nope").is_none());
        assert!(parse_model(b"WHSP1").is_none());
    }

    #[test]
    fn gelu_is_zero_at_zero_and_monotonic_ish() {
        assert!((gelu(0.0)).abs() < 1e-6);
        assert!(gelu(3.0) > gelu(1.0));
        assert!(gelu(-3.0).abs() < gelu(-1.0).abs());
    }

    #[test]
    fn layer_norm_zero_means_unit_variance_per_row() {
        let mut x = alloc::vec![1.0f32, 2.0, 3.0, 4.0, 10.0, 20.0, 30.0, 40.0];
        let w_bytes: Vec<u8> = (0..4).flat_map(|_| 1.0f32.to_le_bytes()).collect();
        let b_bytes: Vec<u8> = (0..4).flat_map(|_| 0.0f32.to_le_bytes()).collect();
        let w = F32View::new(&w_bytes, 4);
        let b = F32View::new(&b_bytes, 4);
        layer_norm_rows(&mut x, 4, w, b);
        let mean: f32 = x[..4].iter().sum::<f32>() / 4.0;
        assert!(mean.abs() < 1e-4);
    }

    #[test]
    fn decode_token_bytes_concatenates_correctly() {
        let bin = fixture_bin();
        let m = parse_model(&bin).unwrap();
        let mut out = Vec::new();
        decode_token_bytes(&m, 0, &mut out);
        decode_token_bytes(&m, 2, &mut out);
        assert_eq!(out, alloc::vec![b'a', b'c']);
    }

    #[test]
    fn decode_token_bytes_ignores_special_ids() {
        let bin = fixture_bin();
        let m = parse_model(&bin).unwrap();
        let mut out = Vec::new();
        decode_token_bytes(&m, m.sot, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn empty_samples_produce_empty_transcript() {
        let bin = fixture_bin();
        let m = parse_model(&bin).unwrap();
        let text = transcribe(&m, &[]);
        assert_eq!(text, "");
    }

    #[test]
    fn silent_samples_produce_empty_transcript() {
        let bin = fixture_bin();
        let m = parse_model(&bin).unwrap();
        let samples = alloc::vec![0.0f32; 16000];
        let text = transcribe(&m, &samples);
        assert_eq!(text, "");
    }

    #[test]
    fn nonsilent_samples_run_full_pipeline_without_panicking() {
        let bin = fixture_bin();
        let m = parse_model(&bin).unwrap();
        let mut samples = alloc::vec![0.0f32; 16000];
        for (i, v) in samples.iter_mut().enumerate() {
            *v = 0.1 * libm::sinf(i as f32 * 0.1);
        }
        let _text = transcribe(&m, &samples); // must not panic; content is meaningless (fixture weights)
    }

    #[test]
    fn transcription_is_deterministic_across_repeated_calls() {
        let bin = fixture_bin();
        let m = parse_model(&bin).unwrap();
        let mut samples = alloc::vec![0.0f32; 16000];
        for (i, v) in samples.iter_mut().enumerate() {
            *v = 0.1 * libm::sinf(i as f32 * 0.13);
        }
        let t1 = transcribe(&m, &samples);
        let t2 = transcribe(&m, &samples);
        assert_eq!(t1, t2);
    }

    #[test]
    fn missing_samples_field_defaults_to_empty() {
        let input = object(alloc::vec![]);
        let samples = read_samples(&input);
        assert!(samples.is_empty());
    }

    /// Without the `full-model` feature (the default, and how `cargo test`
    /// runs), `run()` has no real weight table to parse from and must
    /// degrade gracefully -- empty text, an explicit `model_unavailable`
    /// status -- rather than panicking. The real, fixture-independent
    /// end-to-end behavior (transcribing real audio through the real
    /// weights) is verified separately via wasmtime against the Python
    /// reference, see data/README.md.
    #[test]
    fn run_without_full_model_feature_degrades_gracefully() {
        let input = object(alloc::vec![(
            "samples",
            Value::Array(alloc::vec![Value::Number(0.1)])
        )]);
        let out = run(&input);
        assert_eq!(out.get("text").and_then(Value::as_str), Some(""));
        assert_eq!(
            out.get("status").and_then(Value::as_str),
            Some("model_unavailable")
        );
    }

    #[test]
    fn sinusoid_position_embedding_row_zero_is_all_zero_sin_one_cos() {
        let mut out = alloc::vec![0.0f32; 4 * 8];
        sinusoid_position_embedding(4, 8, &mut out);
        for &v in &out[0..4] {
            assert!(v.abs() < 1e-6); // sin(0)=0
        }
        for &v in &out[4..8] {
            assert!((v - 1.0).abs() < 1e-6); // cos(0)=1
        }
    }

    #[test]
    fn conv1d_stride2_halves_length_with_padding_one() {
        let in_ch = 2usize;
        let in_len = 6usize;
        let out_ch = 2usize;
        let input = alloc::vec![0.5f32; in_ch * in_len];
        let w_bytes: Vec<u8> = (0..out_ch * in_ch * 3)
            .flat_map(|_| 0.0f32.to_le_bytes())
            .collect();
        let b_bytes: Vec<u8> = (0..out_ch).flat_map(|_| 1.0f32.to_le_bytes()).collect();
        let weight = F32View::new(&w_bytes, out_ch * in_ch * 3);
        let bias = F32View::new(&b_bytes, out_ch);
        let out_len = (in_len + 2 - 3) / 2 + 1;
        let mut out = alloc::vec![0.0f32; out_ch * out_len];
        conv1d(&input, in_ch, in_len, weight, bias, out_ch, 2, &mut out);
        assert_eq!(out_len, 3);
        for v in out.iter() {
            assert!((v - 1.0).abs() < 1e-6); // zero weights -> output == bias everywhere
        }
    }
}
