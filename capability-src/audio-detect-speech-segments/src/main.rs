//! Silero VAD (16k) single-chunk speech-risk scorer for
//! `audio.detect-speech-segments`.
//!
//! One capability call scores exactly one 512-sample (32ms) window of 16kHz
//! mono PCM, threading the model's own streaming state (a 64-sample audio
//! "context" carried from the previous window, plus an LSTM `(h, c)` pair)
//! explicitly through the request/response -- the caller chunks a real
//! recording and accumulates probabilities into speech-risk segments (its
//! own policy, e.g. threshold + hysteresis, matching how `classifier.
//! release-gate-evaluate` / `classification.outcome-resolve` keep scoring
//! separate from policy). This mirrors Silero's own upstream `OnnxWrapper.
//! __call__` state machine exactly (verified against it -- see
//! `scripts/model/prepare_silero_vad_f32.py` and the publish PR).
//!
//! registry#460. Model: `snakers4/silero-vad` (MIT). Architecture (from the
//! model's own ONNX graph, not assumed): reflect-pad-right(64) -> a fixed
//! STFT-as-Conv1d front end (258 out channels = 129 real + 129 imaginary,
//! kernel 256, stride 128) -> magnitude spectrum -> 4x Conv1d+ReLU encoder
//! (129->128->64->64->128 channels) -> one `LSTMCell` step (hidden 128) ->
//! Conv1d(128->1) -> sigmoid.
//!
//! **Not quantized**, unlike `report.summarize-semantic` /
//! `report.translate-fr-semantic` -- verified empirically before choosing:
//! the whole model is already ~1.2 MB float32 (quantizing saves little), and
//! per-tensor int8 on the LSTM gates measurably breaks output (the hidden
//! state carries quantization error forward across every subsequent chunk,
//! unlike a sentence-embedding mean-pool where per-weight noise averages
//! out) -- max|probability diff| ~0.9 against the real ONNX graph over a
//! 20-chunk sequence, vs ~3e-6 (pure float rounding) unquantized over 40.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use wasi_capability_runtime::{object, Value};

#[cfg(feature = "full-model")]
static FULL_MODEL_BIN: &[u8] = include_bytes!("../data/silero-vad-16k-f32.bin");

// Fixed Silero VAD (16k) architecture -- not configurable per export, so
// these are constants rather than binary-format header fields.
const SAMPLES: usize = 512;
const CONTEXT: usize = 64;
const CHUNK: usize = SAMPLES + CONTEXT; // 576, fed to the STFT stage
const NFFT: usize = 256;
const HOP: usize = 128;
const MAG_BINS: usize = NFFT / 2 + 1; // 129
const STFT_OUT: usize = 2 * MAG_BINS; // 258 (real + imaginary halves)
const HIDDEN: usize = 128;

fn f32_le(b: &[u8]) -> f32 {
    f32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn parse_f32_vec(
    bytes: &[u8],
    offset: usize,
    count: usize,
) -> Result<(Vec<f32>, usize), &'static str> {
    let end = offset
        .checked_add(count.checked_mul(4).ok_or("overflow")?)
        .ok_or("overflow")?;
    if bytes.len() < end {
        return Err("truncated tensor");
    }
    let mut out = Vec::with_capacity(count);
    let mut cursor = offset;
    for _ in 0..count {
        out.push(f32_le(&bytes[cursor..cursor + 4]));
        cursor += 4;
    }
    Ok((out, end))
}

struct Model {
    stft_weight: Vec<f32>, // (258, 1, 256)
    conv1_w: Vec<f32>,     // (128, 129, 3)
    conv1_b: Vec<f32>,
    conv2_w: Vec<f32>, // (64, 128, 3)
    conv2_b: Vec<f32>,
    conv3_w: Vec<f32>, // (64, 64, 3)
    conv3_b: Vec<f32>,
    conv4_w: Vec<f32>, // (128, 64, 3)
    conv4_b: Vec<f32>,
    lstm_wih: Vec<f32>, // (512, 128)
    lstm_bih: Vec<f32>,
    lstm_whh: Vec<f32>, // (512, 128)
    lstm_bhh: Vec<f32>,
    final_w: Vec<f32>, // (1, 128, 1)
    final_b: Vec<f32>,
}

impl Model {
    fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < 8 || &bytes[0..4] != b"VAD1" {
            return Err("bad magic");
        }
        let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if version != 1 {
            return Err("bad version");
        }
        let h = 8usize;
        let (stft_weight, h) = parse_f32_vec(bytes, h, STFT_OUT * 1 * NFFT)?;
        let (conv1_w, h) = parse_f32_vec(bytes, h, 128 * MAG_BINS * 3)?;
        let (conv1_b, h) = parse_f32_vec(bytes, h, 128)?;
        let (conv2_w, h) = parse_f32_vec(bytes, h, 64 * 128 * 3)?;
        let (conv2_b, h) = parse_f32_vec(bytes, h, 64)?;
        let (conv3_w, h) = parse_f32_vec(bytes, h, 64 * 64 * 3)?;
        let (conv3_b, h) = parse_f32_vec(bytes, h, 64)?;
        let (conv4_w, h) = parse_f32_vec(bytes, h, 128 * 64 * 3)?;
        let (conv4_b, h) = parse_f32_vec(bytes, h, 128)?;
        let (lstm_wih, h) = parse_f32_vec(bytes, h, 4 * HIDDEN * HIDDEN)?;
        let (lstm_bih, h) = parse_f32_vec(bytes, h, 4 * HIDDEN)?;
        let (lstm_whh, h) = parse_f32_vec(bytes, h, 4 * HIDDEN * HIDDEN)?;
        let (lstm_bhh, h) = parse_f32_vec(bytes, h, 4 * HIDDEN)?;
        let (final_w, h) = parse_f32_vec(bytes, h, HIDDEN)?;
        let (final_b, _h) = parse_f32_vec(bytes, h, 1)?;
        Ok(Model {
            stft_weight,
            conv1_w,
            conv1_b,
            conv2_w,
            conv2_b,
            conv3_w,
            conv3_b,
            conv4_w,
            conv4_b,
            lstm_wih,
            lstm_bih,
            lstm_whh,
            lstm_bhh,
            final_w,
            final_b,
        })
    }
}

// ---------------------------------------------------------------------
// Math: generic Conv1d over channel-major (Vec<Vec<f32>>) activations, plus
// the STFT-as-conv front end, the LSTM cell step, and the final classifier.
// ---------------------------------------------------------------------

/// `x`: `c_in` rows of `t` samples each. `weight` flat row-major
/// `(c_out, c_in, k)` (PyTorch/ONNX Conv1d layout). Zero-pads `pad` on both
/// sides before convolving; `bias` is per-output-channel, added when present.
fn conv1d(
    x: &[Vec<f32>],
    weight: &[f32],
    c_out: usize,
    c_in: usize,
    k: usize,
    bias: Option<&[f32]>,
    stride: usize,
    pad: usize,
) -> Vec<Vec<f32>> {
    let t = x[0].len();
    let t_pad = t + 2 * pad;
    let mut xp: Vec<Vec<f32>> = Vec::with_capacity(c_in);
    for ch in x {
        let mut row = alloc::vec![0f32; t_pad];
        row[pad..pad + t].copy_from_slice(ch);
        xp.push(row);
    }
    let t_out = (t_pad - k) / stride + 1;
    let mut out: Vec<Vec<f32>> = alloc::vec![alloc::vec![0f32; t_out]; c_out];
    for oc in 0..c_out {
        let b = bias.map(|b| b[oc]).unwrap_or(0.0);
        for ti in 0..t_out {
            let start = ti * stride;
            let mut acc = b;
            for ic in 0..c_in {
                let wbase = oc * c_in * k + ic * k;
                for kk in 0..k {
                    acc += weight[wbase + kk] * xp[ic][start + kk];
                }
            }
            out[oc][ti] = acc;
        }
    }
    out
}

fn relu_inplace(x: &mut [Vec<f32>]) {
    for row in x.iter_mut() {
        for v in row.iter_mut() {
            if *v < 0.0 {
                *v = 0.0;
            }
        }
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + libm::expf(-x))
}

/// Reflect-pad `pad` samples onto the right of `x` only (verified against
/// the real ONNX graph's intermediate `Pad` output -- leading samples are
/// untouched, trailing padded samples mirror the tail, excluding the edge
/// sample itself: `padded[n+j] = x[n-2-j]`).
fn reflect_pad_right(x: &[f32], pad: usize) -> Vec<f32> {
    let n = x.len();
    let mut out = Vec::with_capacity(n + pad);
    out.extend_from_slice(x);
    for j in 0..pad {
        out.push(x[n - 2 - j]);
    }
    out
}

fn stft_magnitude(model: &Model, x_chunk: &[f32]) -> Vec<Vec<f32>> {
    let padded = reflect_pad_right(x_chunk, CONTEXT); // 576 -> 640
    let x_row = alloc::vec![padded];
    let conv = conv1d(&x_row, &model.stft_weight, STFT_OUT, 1, NFFT, None, HOP, 0); // (258, T)
    let t = conv[0].len();
    let mut mag: Vec<Vec<f32>> = alloc::vec![alloc::vec![0f32; t]; MAG_BINS];
    for bin in 0..MAG_BINS {
        for ti in 0..t {
            let re = conv[bin][ti];
            let im = conv[bin + MAG_BINS][ti];
            mag[bin][ti] = libm::sqrtf(re * re + im * im);
        }
    }
    mag
}

fn encoder(model: &Model, mag: Vec<Vec<f32>>) -> Vec<Vec<f32>> {
    let mut x = conv1d(
        &mag,
        &model.conv1_w,
        128,
        MAG_BINS,
        3,
        Some(&model.conv1_b),
        1,
        1,
    );
    relu_inplace(&mut x);
    let mut x = conv1d(&x, &model.conv2_w, 64, 128, 3, Some(&model.conv2_b), 2, 1);
    relu_inplace(&mut x);
    let mut x = conv1d(&x, &model.conv3_w, 64, 64, 3, Some(&model.conv3_b), 2, 1);
    relu_inplace(&mut x);
    let mut x = conv1d(&x, &model.conv4_w, 128, 64, 3, Some(&model.conv4_b), 1, 1);
    relu_inplace(&mut x);
    x // (128, T')
}

/// One `LSTMCell` step. PyTorch gate order in the packed `4*hidden` weight
/// rows is `(input, forget, cell/candidate, output)` -- verified against the
/// real ONNX graph's output (exact match, see the publish PR).
fn lstm_cell_step(model: &Model, x_t: &[f32], h: &[f32], c: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let mut gates = alloc::vec![0f32; 4 * HIDDEN];
    for row in 0..4 * HIDDEN {
        let mut acc = model.lstm_bih[row] + model.lstm_bhh[row];
        let wih_base = row * HIDDEN;
        let whh_base = row * HIDDEN;
        for col in 0..HIDDEN {
            acc += model.lstm_wih[wih_base + col] * x_t[col];
            acc += model.lstm_whh[whh_base + col] * h[col];
        }
        gates[row] = acc;
    }
    let mut h_new = alloc::vec![0f32; HIDDEN];
    let mut c_new = alloc::vec![0f32; HIDDEN];
    for i in 0..HIDDEN {
        let gi = sigmoid(gates[i]);
        let gf = sigmoid(gates[HIDDEN + i]);
        let gg = libm::tanhf(gates[2 * HIDDEN + i]);
        let go = sigmoid(gates[3 * HIDDEN + i]);
        let cn = gf * c[i] + gi * gg;
        c_new[i] = cn;
        h_new[i] = go * libm::tanhf(cn);
    }
    (h_new, c_new)
}

fn decode_step(model: &Model, h: &[f32]) -> f32 {
    let relu_h: Vec<f32> = h.iter().map(|v| if *v < 0.0 { 0.0 } else { *v }).collect();
    let mut acc = model.final_b[0];
    for i in 0..HIDDEN {
        acc += model.final_w[i] * relu_h[i];
    }
    sigmoid(acc)
}

/// One full model step: 512 new samples + carried context/state in, speech
/// probability + updated context/state out.
fn score_chunk(
    model: &Model,
    samples: &[f32],
    context: &[f32],
    h: &[f32],
    c: &[f32],
) -> (f32, Vec<f32>, Vec<f32>, Vec<f32>) {
    let mut chunk = Vec::with_capacity(CHUNK);
    chunk.extend_from_slice(context);
    chunk.extend_from_slice(samples);

    let mag = stft_magnitude(model, &chunk);
    let enc = encoder(model, mag); // (128, T')
    let t_prime = enc[0].len();

    let mut h_cur = h.to_vec();
    let mut c_cur = c.to_vec();
    let mut sum = 0f32;
    for t in 0..t_prime {
        let x_t: Vec<f32> = (0..HIDDEN).map(|d| enc[d][t]).collect();
        let (h_next, c_next) = lstm_cell_step(model, &x_t, &h_cur, &c_cur);
        sum += decode_step(model, &h_next);
        h_cur = h_next;
        c_cur = c_next;
    }
    let prob = if t_prime > 0 {
        sum / t_prime as f32
    } else {
        0.0
    };
    let new_context = chunk[chunk.len() - CONTEXT..].to_vec();
    (prob, new_context, h_cur, c_cur)
}

// ---------------------------------------------------------------------
// Capability request/response wiring.
// ---------------------------------------------------------------------

fn f32_array(items: &[f32]) -> Value {
    Value::Array(items.iter().map(|v| Value::Number(*v as f64)).collect())
}

/// Returns `None` if the field is absent (caller supplies the default);
/// `Some(Err(()))` if present but malformed (wrong length or non-numeric) --
/// distinguished from "absent" so absence can default while malformed input
/// still fails closed.
fn read_f32_array(input: &Value, key: &str, expected_len: usize) -> Option<Result<Vec<f32>, ()>> {
    let field = input.get(key)?;
    let items = match field.as_array() {
        Some(items) => items,
        None => return Some(Err(())),
    };
    if items.len() != expected_len {
        return Some(Err(()));
    }
    let mut out = Vec::with_capacity(expected_len);
    for item in items {
        match item.as_f64() {
            Some(v) => out.push(v as f32),
            None => return Some(Err(())),
        }
    }
    Some(Ok(out))
}

fn fail_closed_response() -> Value {
    object(alloc::vec![
        ("status", Value::String(String::from("invalid_input"))),
        // Maximally cautious: an unscoreable chunk is treated as containing
        // speech, never as verified-safe. A downstream policy (e.g.
        // classifier.release-gate-evaluate) MUST also check `status`, not
        // just threshold this probability, but a probability-only consumer
        // still fails safe.
        ("speech_probability", Value::Number(1.0)),
        ("context", f32_array(&alloc::vec![0f32; CONTEXT])),
        ("state_h", f32_array(&alloc::vec![0f32; HIDDEN])),
        ("state_c", f32_array(&alloc::vec![0f32; HIDDEN])),
    ])
}

fn detect(model: &Model, input: Value) -> Value {
    let samples = match read_f32_array(&input, "samples", SAMPLES) {
        Some(Ok(v)) => v,
        _ => return fail_closed_response(),
    };
    let context = match read_f32_array(&input, "context", CONTEXT) {
        Some(Ok(v)) => v,
        Some(Err(())) => return fail_closed_response(),
        None => alloc::vec![0f32; CONTEXT],
    };
    let state_h = match read_f32_array(&input, "state_h", HIDDEN) {
        Some(Ok(v)) => v,
        Some(Err(())) => return fail_closed_response(),
        None => alloc::vec![0f32; HIDDEN],
    };
    let state_c = match read_f32_array(&input, "state_c", HIDDEN) {
        Some(Ok(v)) => v,
        Some(Err(())) => return fail_closed_response(),
        None => alloc::vec![0f32; HIDDEN],
    };

    let (prob, new_context, new_h, new_c) =
        score_chunk(model, &samples, &context, &state_h, &state_c);
    let prob = if prob.is_finite() {
        prob.clamp(0.0, 1.0)
    } else {
        1.0
    }; // fail closed on non-finite too

    object(alloc::vec![
        ("status", Value::String(String::from("ok"))),
        ("speech_probability", Value::Number(prob as f64)),
        ("context", f32_array(&new_context)),
        ("state_h", f32_array(&new_h)),
        ("state_c", f32_array(&new_c)),
    ])
}

#[cfg(all(feature = "full-model", not(test)))]
fn load_production_model() -> Model {
    match Model::parse(FULL_MODEL_BIN) {
        Ok(model) => model,
        Err(_) => loop {
            // Pinned artifact parse is infallible; abort without panic!.
            core::arch::wasm32::unreachable()
        },
    }
}

#[cfg(all(feature = "full-model", not(test)))]
fn run(input: Value) -> Value {
    let model = load_production_model();
    detect(&model, input)
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    #[cfg(feature = "full-model")]
    {
        wasi_capability_runtime::run_capability(run);
    }
    #[cfg(not(feature = "full-model"))]
    {
        // Release builds must enable `full-model`.
        loop {
            core::arch::wasm32::unreachable()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------
    // Deterministic-but-varied synthetic VAD1 fixture at the REAL fixed
    // dimensions (this model's architecture isn't configurable, so a
    // "tiny" fixture isn't smaller than the real one) -- exercises the
    // parser/conv/LSTM/sigmoid plumbing. NOT a claim of real VAD accuracy;
    // that's verified separately against the published `full-model`
    // artifact and the real ONNX graph (see the publish PR).
    // ---------------------------------------------------------------

    fn varied(len: usize, seed: i32, scale: f32) -> Vec<f32> {
        (0..len)
            .map(|i| {
                let v = ((i as i32 * 7 + seed) % 13) - 6; // in [-6, 6], varied not constant
                v as f32 * scale
            })
            .collect()
    }

    fn fixture_bin() -> Vec<u8> {
        let mut bin = Vec::new();
        bin.extend_from_slice(b"VAD1");
        bin.extend_from_slice(&1u32.to_le_bytes());
        let push_f32s = |bin: &mut Vec<u8>, v: &[f32]| {
            for x in v {
                bin.extend_from_slice(&x.to_le_bytes());
            }
        };
        push_f32s(&mut bin, &varied(STFT_OUT * NFFT, 1, 0.01));
        push_f32s(&mut bin, &varied(128 * MAG_BINS * 3, 2, 0.02));
        push_f32s(&mut bin, &varied(128, 3, 0.01));
        push_f32s(&mut bin, &varied(64 * 128 * 3, 4, 0.02));
        push_f32s(&mut bin, &varied(64, 5, 0.01));
        push_f32s(&mut bin, &varied(64 * 64 * 3, 6, 0.02));
        push_f32s(&mut bin, &varied(64, 7, 0.01));
        push_f32s(&mut bin, &varied(128 * 64 * 3, 8, 0.02));
        push_f32s(&mut bin, &varied(128, 9, 0.01));
        push_f32s(&mut bin, &varied(4 * HIDDEN * HIDDEN, 10, 0.005));
        push_f32s(&mut bin, &varied(4 * HIDDEN, 11, 0.01));
        push_f32s(&mut bin, &varied(4 * HIDDEN * HIDDEN, 12, 0.005));
        push_f32s(&mut bin, &varied(4 * HIDDEN, 13, 0.01));
        push_f32s(&mut bin, &varied(HIDDEN, 14, 0.02));
        push_f32s(&mut bin, &varied(1, 15, 0.01));
        bin
    }

    fn model() -> Model {
        Model::parse(&fixture_bin()).expect("fixture parses")
    }

    fn call(model: &Model, json: &str) -> Value {
        let input = wasi_capability_runtime::parse_json(json).expect("parse");
        detect(model, input)
    }

    fn samples_json(fill: &[f32]) -> String {
        let mut s = String::from("[");
        for (i, v) in fill.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&alloc::format!("{v}"));
        }
        s.push(']');
        s
    }

    #[test]
    fn parses_fixture_and_reports_correct_tensor_sizes() {
        let m = model();
        assert_eq!(m.stft_weight.len(), STFT_OUT * NFFT);
        assert_eq!(m.conv1_w.len(), 128 * MAG_BINS * 3);
        assert_eq!(m.lstm_wih.len(), 4 * HIDDEN * HIDDEN);
        assert_eq!(m.final_w.len(), HIDDEN);
    }

    #[test]
    fn bad_magic_is_rejected() {
        let mut bad = fixture_bin();
        bad[0] = b'X';
        assert!(Model::parse(&bad).is_err());
    }

    #[test]
    fn reflect_pad_right_mirrors_tail_without_repeating_edge() {
        let x = [1.0f32, 2.0, 3.0, 4.0, 5.0];
        let padded = reflect_pad_right(&x, 3);
        // padded = [1,2,3,4,5, 4,3,2]  (mirror of indices n-2, n-3, n-4)
        assert_eq!(padded, alloc::vec![1.0, 2.0, 3.0, 4.0, 5.0, 4.0, 3.0, 2.0]);
    }

    #[test]
    fn score_chunk_produces_finite_probability_in_unit_range() {
        let m = model();
        let samples = varied(SAMPLES, 100, 0.3);
        let context = alloc::vec![0f32; CONTEXT];
        let h = alloc::vec![0f32; HIDDEN];
        let c = alloc::vec![0f32; HIDDEN];
        let (prob, new_ctx, new_h, new_c) = score_chunk(&m, &samples, &context, &h, &c);
        assert!(prob.is_finite());
        assert!((0.0..=1.0).contains(&prob));
        assert_eq!(new_ctx.len(), CONTEXT);
        assert_eq!(new_h.len(), HIDDEN);
        assert_eq!(new_c.len(), HIDDEN);
    }

    #[test]
    fn score_chunk_is_deterministic() {
        let m = model();
        let samples = varied(SAMPLES, 55, 0.2);
        let ctx = alloc::vec![0f32; CONTEXT];
        let h = alloc::vec![0f32; HIDDEN];
        let c = alloc::vec![0f32; HIDDEN];
        let a = score_chunk(&m, &samples, &ctx, &h, &c);
        let b = score_chunk(&m, &samples, &ctx, &h, &c);
        assert_eq!(a.0, b.0);
        assert_eq!(a.1, b.1);
    }

    #[test]
    fn score_chunk_differs_for_different_input() {
        let m = model();
        let ctx = alloc::vec![0f32; CONTEXT];
        let h = alloc::vec![0f32; HIDDEN];
        let c = alloc::vec![0f32; HIDDEN];
        let a = score_chunk(&m, &varied(SAMPLES, 1, 0.2), &ctx, &h, &c);
        let b = score_chunk(&m, &varied(SAMPLES, 999, 0.2), &ctx, &h, &c);
        assert_ne!(a.0, b.0);
    }

    #[test]
    fn context_carries_last_samples_of_the_full_576_window() {
        let m = model();
        let samples = varied(SAMPLES, 7, 1.0);
        let ctx = alloc::vec![0f32; CONTEXT];
        let h = alloc::vec![0f32; HIDDEN];
        let c = alloc::vec![0f32; HIDDEN];
        let (_prob, new_ctx, _h, _c) = score_chunk(&m, &samples, &ctx, &h, &c);
        // new_context is exactly the last CONTEXT samples of the new chunk.
        assert_eq!(new_ctx, samples[SAMPLES - CONTEXT..].to_vec());
    }

    #[test]
    fn detect_wrong_length_samples_fails_closed() {
        let m = model();
        let out = call(&m, r#"{"samples":[1.0,2.0]}"#);
        assert_eq!(
            out.get("status").unwrap().as_str().unwrap(),
            "invalid_input"
        );
        assert_eq!(
            out.get("speech_probability").unwrap().as_f64().unwrap(),
            1.0
        );
    }

    #[test]
    fn detect_missing_samples_fails_closed() {
        let m = model();
        let out = call(&m, r#"{}"#);
        assert_eq!(
            out.get("status").unwrap().as_str().unwrap(),
            "invalid_input"
        );
    }

    #[test]
    fn detect_malformed_context_fails_closed_even_with_valid_samples() {
        let m = model();
        let samples = varied(SAMPLES, 3, 0.1);
        let json = alloc::format!(
            r#"{{"samples":{},"context":[1.0,2.0]}}"#,
            samples_json(&samples)
        );
        let out = call(&m, &json);
        assert_eq!(
            out.get("status").unwrap().as_str().unwrap(),
            "invalid_input"
        );
    }

    #[test]
    fn detect_ok_path_defaults_context_and_state_when_absent() {
        let m = model();
        let samples = varied(SAMPLES, 42, 0.15);
        let json = alloc::format!(r#"{{"samples":{}}}"#, samples_json(&samples));
        let out = call(&m, &json);
        assert_eq!(out.get("status").unwrap().as_str().unwrap(), "ok");
        let prob = out.get("speech_probability").unwrap().as_f64().unwrap();
        assert!((0.0..=1.0).contains(&prob));
        assert_eq!(
            out.get("context").unwrap().as_array().unwrap().len(),
            CONTEXT
        );
        assert_eq!(
            out.get("state_h").unwrap().as_array().unwrap().len(),
            HIDDEN
        );
        assert_eq!(
            out.get("state_c").unwrap().as_array().unwrap().len(),
            HIDDEN
        );
    }

    #[test]
    fn detect_threads_returned_context_and_state_into_a_second_call() {
        let m = model();
        let samples1 = varied(SAMPLES, 5, 0.1);
        let json1 = alloc::format!(r#"{{"samples":{}}}"#, samples_json(&samples1));
        let out1 = call(&m, &json1);
        let ctx1 = out1.get("context").unwrap();
        let h1 = out1.get("state_h").unwrap();
        let c1 = out1.get("state_c").unwrap();

        let samples2 = varied(SAMPLES, 6, 0.1);
        let json2 = alloc::format!(
            r#"{{"samples":{},"context":{},"state_h":{},"state_c":{}}}"#,
            samples_json(&samples2),
            wasi_capability_runtime::write_json(ctx1),
            wasi_capability_runtime::write_json(h1),
            wasi_capability_runtime::write_json(c1),
        );
        let out2 = call(&m, &json2);
        assert_eq!(out2.get("status").unwrap().as_str().unwrap(), "ok");
    }

    #[test]
    fn f32_array_and_read_f32_array_round_trip() {
        let vals = alloc::vec![1.5f32, -2.25, 0.0];
        let v = f32_array(&vals);
        let json = wasi_capability_runtime::write_json(&v);
        let input = object(alloc::vec![(
            "x",
            wasi_capability_runtime::parse_json(&json).unwrap()
        )]);
        let back = read_f32_array(&input, "x", 3).unwrap().unwrap();
        assert_eq!(back, vals);
    }

    #[test]
    fn read_f32_array_absent_field_returns_none() {
        let input = object(alloc::vec![]);
        assert!(read_f32_array(&input, "missing", 4).is_none());
    }

    #[test]
    fn sigmoid_bounds_and_midpoint() {
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-6);
        assert!(sigmoid(50.0) > 0.999);
        assert!(sigmoid(-50.0) < 0.001);
    }
}
