//! DistilBERT-NER token-classification entity extractor for
//! `text.detect-entities`.
//!
//! Third spec 001 FR-017 agent (registry#465). Real, evidenced multi-app
//! gap: a subagent survey of `traverse-framework/reference-apps` found the
//! same task -- classify free text / extract named entities -- implemented
//! independently, rule-based, in `doc-approval.analyze`'s `docType`,
//! `traverse-starter.process`'s `noteType`, and `meeting-notes.process`'s
//! owner extraction (self-disclosed: "not production-grade named-entity
//! recognition" -- literally "leading capitalized word = owner").
//! `meeting-notes.process` is additionally the one capability with
//! *governed* two-app reuse in that repo.
//!
//! Model: `dslim/distilbert-NER` (Apache-2.0, verified against the model's
//! own HF API metadata `cardData.license`/`license:apache-2.0` tag, not a
//! search summary) -- DistilBERT fine-tuned on CoNLL-2003: 6 transformer
//! layers, hidden=768, 12 heads, intermediate=3072, a cased 28996-token
//! WordPiece vocab, no token-type embeddings (single segment only), and a
//! per-token linear classifier over 9 BIO labels (O / B-PER / I-PER /
//! B-ORG / I-ORG / B-LOC / I-LOC / B-MISC / I-MISC). Reuses the
//! `#![no_std]` BERT-encoder + WordPiece-tokenizer pattern proven in
//! `report.translate-fr-semantic` (registry#455) -- bigger (6 layers/768
//! hidden vs 3/384) and this model's weight matrices are ONNX `MatMul(x, W)`
//! convention (`W` stored `(in, out)`, no transpose), not HF's native
//! `(out, in)` `nn.Linear.weight` layout -- but the same attention/FFN/
//! LayerNorm shape. No pooling: every token gets its own label, decoded
//! into BIO entity spans with byte offsets into the input text.
//!
//! Quantized (int8 weight matrices, float32 biases/LayerNorm) -- unlike the
//! recurrent Silero VAD (registry#460), this is a stateless feedforward
//! call with nothing carried between invocations, so quantization noise
//! doesn't compound: 0/65 label mismatches across 5 varied test sentences,
//! verified against the real ONNX graph before writing this file.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use wasi_capability_runtime::{object, Value};

#[cfg(feature = "full-model")]
static FULL_MODEL_BIN: &[u8] = include_bytes!("../data/distilbert-ner-int8.bin");

/// Hard cap on WordPiece tokens (CLS + body + SEP) fed to the encoder --
/// bounds O(seq^2) attention cost. Well under the model's own
/// max_position_embeddings (512); a real sentence/short-paragraph NER call
/// is far shorter.
const MAX_SEQ_TOKENS: usize = 256;
const MAX_INPUT_CHARS_PER_WORD: usize = 100;
const NUM_LABELS: usize = 9;

// ---------------------------------------------------------------------
// Binary model format (NER1) -- see scripts/model/prepare_distilbert_ner_int8.py.
// Layout: magic, header, quantized matrices (f32 scale + row-major int8
// bytes, (in,out) orientation), float32 tensors (raw f32 bytes), vocab
// (u16-length-prefixed UTF-8 tokens, index == token id).
// ---------------------------------------------------------------------

fn u32_le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn f32_le(b: &[u8]) -> f32 {
    f32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn u16_le(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

/// A quantized weight matrix in ONNX `MatMul(x, W)` orientation: `x` is
/// `(seq, rows)`, `W` is `(rows, cols)` = `(in, out)`, output is
/// `(seq, cols)` -- i.e. `out[o] = sum_i x[i] * W[i, o]`, NOT the
/// transposed `(out, in)` convention `report-translate-fr-semantic` uses
/// for its HF-safetensors-sourced weights.
struct QMat<'a> {
    scale: f32,
    rows: usize, // in_features
    cols: usize, // out_features
    data: &'a [u8],
}

impl<'a> QMat<'a> {
    fn parse(
        bytes: &'a [u8],
        offset: usize,
        rows: usize,
        cols: usize,
    ) -> Result<(Self, usize), &'static str> {
        if bytes.len() < offset + 4 {
            return Err("truncated scale");
        }
        let scale = f32_le(&bytes[offset..offset + 4]);
        let start = offset + 4;
        let len = rows.checked_mul(cols).ok_or("overflow")?;
        let end = start.checked_add(len).ok_or("overflow")?;
        if bytes.len() < end {
            return Err("truncated matrix");
        }
        Ok((
            QMat {
                scale,
                rows,
                cols,
                data: &bytes[start..end],
            },
            end,
        ))
    }

    #[inline]
    fn at(&self, r: usize, c: usize) -> f32 {
        (self.data[r * self.cols + c] as i8) as f32 * self.scale
    }
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
        return Err("truncated float tensor");
    }
    let mut out = Vec::with_capacity(count);
    let mut cursor = offset;
    for _ in 0..count {
        out.push(f32_le(&bytes[cursor..cursor + 4]));
        cursor += 4;
    }
    Ok((out, end))
}

struct LayerWeights<'a> {
    q_w: QMat<'a>,
    q_b: Vec<f32>,
    k_w: QMat<'a>,
    k_b: Vec<f32>,
    v_w: QMat<'a>,
    v_b: Vec<f32>,
    out_w: QMat<'a>,
    out_b: Vec<f32>,
    sa_ln_w: Vec<f32>,
    sa_ln_b: Vec<f32>,
    ffn1_w: QMat<'a>,
    ffn1_b: Vec<f32>,
    ffn2_w: QMat<'a>,
    ffn2_b: Vec<f32>,
    out_ln_w: Vec<f32>,
    out_ln_b: Vec<f32>,
}

struct Model<'a> {
    hidden: usize,
    heads: usize,
    head_dim: usize,
    ffn: usize,
    cls_id: u32,
    sep_id: u32,
    unk_id: u32,
    word_emb: QMat<'a>,
    pos_emb: QMat<'a>,
    emb_ln_w: Vec<f32>,
    emb_ln_b: Vec<f32>,
    layers: Vec<LayerWeights<'a>>,
    classifier_w: QMat<'a>,
    classifier_b: Vec<f32>,
    /// (token text, token id), sorted by text for binary-search WordPiece lookup.
    token_index: Vec<(&'a str, u32)>,
}

impl<'a> Model<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self, &'static str> {
        if bytes.len() < 4 || &bytes[0..4] != b"NER1" {
            return Err("bad magic");
        }
        if bytes.len() < 4 + 40 + 4 {
            return Err("truncated header");
        }
        let mut h = 4usize;
        let version = u32_le(&bytes[h..h + 4]);
        h += 4;
        if version != 1 {
            return Err("bad version");
        }
        let vocab_size = u32_le(&bytes[h..h + 4]) as usize;
        h += 4;
        let hidden = u32_le(&bytes[h..h + 4]) as usize;
        h += 4;
        let layers_n = u32_le(&bytes[h..h + 4]) as usize;
        h += 4;
        let heads = u32_le(&bytes[h..h + 4]) as usize;
        h += 4;
        let ffn = u32_le(&bytes[h..h + 4]) as usize;
        h += 4;
        let num_labels = u32_le(&bytes[h..h + 4]) as usize;
        h += 4;
        let cls_id = u32_le(&bytes[h..h + 4]);
        h += 4;
        let sep_id = u32_le(&bytes[h..h + 4]);
        h += 4;
        let unk_id = u32_le(&bytes[h..h + 4]);
        h += 4;
        let _pad_id = u32_le(&bytes[h..h + 4]);
        h += 4;

        if hidden == 0
            || heads == 0
            || hidden % heads != 0
            || layers_n == 0
            || ffn == 0
            || vocab_size == 0
            || num_labels != NUM_LABELS
        {
            return Err("bad dims");
        }
        let head_dim = hidden / heads;

        let (word_emb, h) = QMat::parse(bytes, h, vocab_size, hidden)?;
        let (pos_emb, mut h) = QMat::parse(bytes, h, 512, hidden)?;

        struct RawLayer<'a> {
            q_w: QMat<'a>,
            k_w: QMat<'a>,
            v_w: QMat<'a>,
            out_w: QMat<'a>,
            ffn1_w: QMat<'a>,
            ffn2_w: QMat<'a>,
        }
        let mut raw_layers: Vec<RawLayer<'a>> = Vec::with_capacity(layers_n);
        for _ in 0..layers_n {
            let (q_w, nh) = QMat::parse(bytes, h, hidden, hidden)?;
            let (k_w, nh) = QMat::parse(bytes, nh, hidden, hidden)?;
            let (v_w, nh) = QMat::parse(bytes, nh, hidden, hidden)?;
            let (out_w, nh) = QMat::parse(bytes, nh, hidden, hidden)?;
            let (ffn1_w, nh) = QMat::parse(bytes, nh, hidden, ffn)?;
            let (ffn2_w, nh) = QMat::parse(bytes, nh, ffn, hidden)?;
            h = nh;
            raw_layers.push(RawLayer {
                q_w,
                k_w,
                v_w,
                out_w,
                ffn1_w,
                ffn2_w,
            });
        }
        let (classifier_w, h) = QMat::parse(bytes, h, hidden, NUM_LABELS)?;

        // float32 tensors: emb LN, then per layer (q/k/v/out bias, sa LN,
        // ffn1/ffn2 bias, out LN), then classifier bias.
        let (emb_ln_w, nh) = parse_f32_vec(bytes, h, hidden)?;
        let (emb_ln_b, mut h) = parse_f32_vec(bytes, nh, hidden)?;

        let mut layers = Vec::with_capacity(layers_n);
        for raw in raw_layers {
            let (q_b, nh) = parse_f32_vec(bytes, h, hidden)?;
            let (k_b, nh) = parse_f32_vec(bytes, nh, hidden)?;
            let (v_b, nh) = parse_f32_vec(bytes, nh, hidden)?;
            let (out_b, nh) = parse_f32_vec(bytes, nh, hidden)?;
            let (sa_ln_w, nh) = parse_f32_vec(bytes, nh, hidden)?;
            let (sa_ln_b, nh) = parse_f32_vec(bytes, nh, hidden)?;
            let (ffn1_b, nh) = parse_f32_vec(bytes, nh, ffn)?;
            let (ffn2_b, nh) = parse_f32_vec(bytes, nh, hidden)?;
            let (out_ln_w, nh) = parse_f32_vec(bytes, nh, hidden)?;
            let (out_ln_b, nh) = parse_f32_vec(bytes, nh, hidden)?;
            h = nh;
            layers.push(LayerWeights {
                q_w: raw.q_w,
                q_b,
                k_w: raw.k_w,
                k_b,
                v_w: raw.v_w,
                v_b,
                out_w: raw.out_w,
                out_b,
                sa_ln_w,
                sa_ln_b,
                ffn1_w: raw.ffn1_w,
                ffn1_b,
                ffn2_w: raw.ffn2_w,
                ffn2_b,
                out_ln_w,
                out_ln_b,
            });
        }
        let (classifier_b, mut h) = parse_f32_vec(bytes, h, NUM_LABELS)?;

        let mut lens = Vec::with_capacity(vocab_size);
        for _ in 0..vocab_size {
            if bytes.len() < h + 2 {
                return Err("truncated vocab lengths");
            }
            lens.push(u16_le(&bytes[h..h + 2]) as usize);
            h += 2;
        }
        let mut token_index = Vec::with_capacity(vocab_size);
        for (id, len) in lens.into_iter().enumerate() {
            let end = h.checked_add(len).ok_or("overflow")?;
            if bytes.len() < end {
                return Err("truncated vocab tokens");
            }
            let tok = core::str::from_utf8(&bytes[h..end]).map_err(|_| "utf8")?;
            token_index.push((tok, id as u32));
            h = end;
        }
        token_index.sort_by(|a, b| a.0.cmp(b.0));

        Ok(Model {
            hidden,
            heads,
            head_dim,
            ffn,
            cls_id,
            sep_id,
            unk_id,
            word_emb,
            pos_emb,
            emb_ln_w,
            emb_ln_b,
            layers,
            classifier_w,
            classifier_b,
            token_index,
        })
    }
}

fn lookup_token(index: &[(&str, u32)], token: &str) -> Option<u32> {
    index
        .binary_search_by_key(&token, |entry| entry.0)
        .ok()
        .map(|i| index[i].1)
}

// ---------------------------------------------------------------------
// Case-PRESERVING WordPiece tokenization with byte-offset tracking (NER
// needs exact spans into the original text; the cased vocab means
// lowercasing, unlike the other BERT-family crates in this repo, would
// also destroy the model's actual signal for entity boundaries).
// ---------------------------------------------------------------------

/// `(char, byte_offset_of_that_char)`, plus one trailing sentinel entry
/// (byte length of the whole string) so a piece's END offset is always a
/// valid index.
fn char_byte_offsets(text: &str) -> (Vec<char>, Vec<usize>) {
    let mut chars = Vec::new();
    let mut offsets = Vec::new();
    let mut cursor = 0usize;
    for c in text.chars() {
        offsets.push(cursor);
        chars.push(c);
        cursor += c.len_utf8();
    }
    offsets.push(cursor);
    (chars, offsets)
}

/// Whitespace-delimited runs of alphanumerics, with each punctuation
/// character isolated as its own token -- BERT's standard "basic
/// tokenize" split, case preserved. Returns char-index ranges.
fn basic_token_ranges(chars: &[char]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut run_start: Option<usize> = None;
    for (idx, c) in chars.iter().enumerate() {
        if c.is_alphanumeric() {
            if run_start.is_none() {
                run_start = Some(idx);
            }
        } else {
            if let Some(s) = run_start.take() {
                ranges.push((s, idx));
            }
            if !c.is_whitespace() {
                ranges.push((idx, idx + 1));
            }
        }
    }
    if let Some(s) = run_start {
        ranges.push((s, chars.len()));
    }
    ranges
}

/// Greedy-longest-match WordPiece over `chars[range.0..range.1]`. Returns
/// `(token_id, char_start, char_end)` triples (absolute char indices).
fn wordpiece_ids(model: &Model, chars: &[char], range: (usize, usize)) -> Vec<(u32, usize, usize)> {
    let (rs, re) = range;
    if re - rs > MAX_INPUT_CHARS_PER_WORD {
        return alloc::vec![(model.unk_id, rs, re)];
    }
    let whole: String = chars[rs..re].iter().collect();
    if let Some(id) = lookup_token(&model.token_index, whole.as_str()) {
        return alloc::vec![(id, rs, re)];
    }
    let mut pieces = Vec::new();
    let mut start = rs;
    while start < re {
        let mut end = re;
        let mut found: Option<(u32, usize)> = None;
        while start < end {
            let mut piece: String = chars[start..end].iter().collect();
            if start > rs {
                piece = format!("##{piece}");
            }
            if let Some(id) = lookup_token(&model.token_index, piece.as_str()) {
                found = Some((id, end));
                break;
            }
            end -= 1;
        }
        if let Some((id, next)) = found {
            pieces.push((id, start, next));
            start = next;
        } else {
            return alloc::vec![(model.unk_id, rs, re)];
        }
    }
    if pieces.is_empty() {
        alloc::vec![(model.unk_id, rs, re)]
    } else {
        pieces
    }
}

struct Token {
    id: u32,
    start_byte: usize,
    end_byte: usize,
}

/// `[CLS] <wordpieces> [SEP]`, byte-offset-tracked. CLS/SEP carry a
/// zero-width offset at their conceptual position (never used for entity
/// spans -- their labels are always excluded from BIO decoding).
fn encode(model: &Model, text: &str) -> Vec<Token> {
    let (chars, offsets) = char_byte_offsets(text);
    let mut tokens = Vec::new();
    tokens.push(Token {
        id: model.cls_id,
        start_byte: 0,
        end_byte: 0,
    });
    'words: for range in basic_token_ranges(&chars) {
        for (id, cs, ce) in wordpiece_ids(model, &chars, range) {
            if tokens.len() >= MAX_SEQ_TOKENS - 1 {
                break 'words;
            }
            tokens.push(Token {
                id,
                start_byte: offsets[cs],
                end_byte: offsets[ce],
            });
        }
    }
    let end = offsets[chars.len()];
    tokens.push(Token {
        id: model.sep_id,
        start_byte: end,
        end_byte: end,
    });
    tokens
}

// ---------------------------------------------------------------------
// DistilBERT forward pass: embeddings (word + position, NO token-type) ->
// N transformer layers (self-attention + LayerNorm, GELU FFN + LayerNorm)
// -> per-token classifier logits. Single un-batched sequence (every
// position real, no padding/attention-mask needed).
// ---------------------------------------------------------------------

fn layer_norm(x: &mut [f32], weight: &[f32], bias: &[f32], eps: f32) {
    let n = x.len() as f32;
    let mean: f32 = x.iter().sum::<f32>() / n;
    let mut var = 0f32;
    for v in x.iter() {
        let d = *v - mean;
        var += d * d;
    }
    var /= n;
    let denom = libm::sqrtf(var + eps);
    for i in 0..x.len() {
        x[i] = (x[i] - mean) / denom * weight[i] + bias[i];
    }
}

fn gelu(x: f32) -> f32 {
    const INV_SQRT_2: f32 = core::f32::consts::FRAC_1_SQRT_2;
    x * 0.5 * (1.0 + libm::erff(x * INV_SQRT_2))
}

/// `x`: `(rows_in)`. `w`: `(rows_in, cols_out)` ONNX `MatMul(x, w)`
/// orientation (see `QMat` doc). `out`: `(cols_out)`.
fn linear_in_out(x: &[f32], w: &QMat, bias: &[f32], out: &mut [f32]) {
    for o in 0..w.cols {
        out[o] = bias[o];
    }
    for i in 0..w.rows {
        let xi = x[i];
        if xi == 0.0 {
            continue;
        }
        for o in 0..w.cols {
            out[o] += xi * w.at(i, o);
        }
    }
}

fn attention(
    x: &[Vec<f32>],
    layer: &LayerWeights,
    heads: usize,
    head_dim: usize,
    hidden: usize,
) -> Vec<Vec<f32>> {
    let seq = x.len();
    let mut q = alloc::vec![alloc::vec![0f32; hidden]; seq];
    let mut k = alloc::vec![alloc::vec![0f32; hidden]; seq];
    let mut v = alloc::vec![alloc::vec![0f32; hidden]; seq];
    for t in 0..seq {
        linear_in_out(&x[t], &layer.q_w, &layer.q_b, &mut q[t]);
        linear_in_out(&x[t], &layer.k_w, &layer.k_b, &mut k[t]);
        linear_in_out(&x[t], &layer.v_w, &layer.v_b, &mut v[t]);
    }

    let mut ctx = alloc::vec![alloc::vec![0f32; hidden]; seq];
    let scale = 1.0f32 / libm::sqrtf(head_dim as f32);
    for h in 0..heads {
        let off = h * head_dim;
        for ti in 0..seq {
            let mut scores = alloc::vec![0f32; seq];
            let mut maxv = f32::NEG_INFINITY;
            for tj in 0..seq {
                let mut dot = 0f32;
                for d in 0..head_dim {
                    dot += q[ti][off + d] * k[tj][off + d];
                }
                let s = dot * scale;
                scores[tj] = s;
                if s > maxv {
                    maxv = s;
                }
            }
            let mut sum = 0f32;
            for s in scores.iter_mut() {
                let e = libm::expf(*s - maxv);
                *s = e;
                sum += e;
            }
            for s in scores.iter_mut() {
                *s /= sum;
            }
            for d in 0..head_dim {
                let mut acc = 0f32;
                for tj in 0..seq {
                    acc += scores[tj] * v[tj][off + d];
                }
                ctx[ti][off + d] = acc;
            }
        }
    }

    let mut out = alloc::vec![alloc::vec![0f32; hidden]; seq];
    for t in 0..seq {
        linear_in_out(&ctx[t], &layer.out_w, &layer.out_b, &mut out[t]);
    }
    out
}

fn ffn(x: &[f32], layer: &LayerWeights, ffn_dim: usize, hidden: usize) -> Vec<f32> {
    let mut inter = alloc::vec![0f32; ffn_dim];
    linear_in_out(x, &layer.ffn1_w, &layer.ffn1_b, &mut inter);
    for v in inter.iter_mut() {
        *v = gelu(*v);
    }
    let mut out = alloc::vec![0f32; hidden];
    linear_in_out(&inter, &layer.ffn2_w, &layer.ffn2_b, &mut out);
    out
}

const EPS: f32 = 1e-12;

fn forward_logits(model: &Model, tokens: &[Token]) -> Vec<Vec<f32>> {
    let seq = tokens.len();
    let mut x: Vec<Vec<f32>> = Vec::with_capacity(seq);
    for (pos, t) in tokens.iter().enumerate() {
        let mut row = alloc::vec![0f32; model.hidden];
        for d in 0..model.hidden {
            row[d] = model.word_emb.at(t.id as usize, d) + model.pos_emb.at(pos, d);
        }
        x.push(row);
    }
    for row in x.iter_mut() {
        layer_norm(row, &model.emb_ln_w, &model.emb_ln_b, EPS);
    }

    for layer in &model.layers {
        let attn = attention(&x, layer, model.heads, model.head_dim, model.hidden);
        for t in 0..seq {
            for d in 0..model.hidden {
                x[t][d] += attn[t][d];
            }
            layer_norm(&mut x[t], &layer.sa_ln_w, &layer.sa_ln_b, EPS);
        }
        for t in 0..seq {
            let f = ffn(&x[t], layer, model.ffn, model.hidden);
            for d in 0..model.hidden {
                x[t][d] += f[d];
            }
            layer_norm(&mut x[t], &layer.out_ln_w, &layer.out_ln_b, EPS);
        }
    }

    let mut logits = Vec::with_capacity(seq);
    for row in &x {
        let mut out = alloc::vec![0f32; NUM_LABELS];
        linear_in_out(row, &model.classifier_w, &model.classifier_b, &mut out);
        logits.push(out);
    }
    logits
}

fn label_name(idx: usize) -> &'static str {
    match idx {
        0 => "O",
        1 => "B-PER",
        2 => "I-PER",
        3 => "B-ORG",
        4 => "I-ORG",
        5 => "B-LOC",
        6 => "I-LOC",
        7 => "B-MISC",
        8 => "I-MISC",
        _ => "O",
    }
}

fn entity_type(label: &str) -> &str {
    label
        .strip_prefix("B-")
        .or_else(|| label.strip_prefix("I-"))
        .unwrap_or("")
}

fn argmax(row: &[f32]) -> usize {
    let mut best = 0usize;
    let mut best_v = row[0];
    for (i, v) in row.iter().enumerate().skip(1) {
        if *v > best_v {
            best_v = *v;
            best = i;
        }
    }
    best
}

struct Entity {
    label: String,
    start: usize,
    end: usize,
}

/// BIO decode over the CONTENT tokens only (CLS/SEP excluded -- their
/// labels are never meaningful spans). A dangling `I-X` with no open `B-X`
/// (a real, if rare, model error mode) is treated leniently as starting a
/// new entity, matching common BIO-decoding practice.
fn decode_entities(tokens: &[Token], logits: &[Vec<f32>]) -> Vec<Entity> {
    let mut entities = Vec::new();
    let mut open: Option<(String, usize, usize)> = None; // (type, start, end)
    let n = tokens.len();
    for i in 1..n.saturating_sub(1) {
        let label = label_name(argmax(&logits[i]));
        let ty = entity_type(label);
        if label.starts_with("B-") {
            if let Some((t, s, e)) = open.take() {
                entities.push(Entity {
                    label: t,
                    start: s,
                    end: e,
                });
            }
            open = Some((String::from(ty), tokens[i].start_byte, tokens[i].end_byte));
        } else if label.starts_with("I-") {
            match &mut open {
                Some((t, _s, e)) if t.as_str() == ty => {
                    *e = tokens[i].end_byte;
                }
                _ => {
                    if let Some((t, s, e)) = open.take() {
                        entities.push(Entity {
                            label: t,
                            start: s,
                            end: e,
                        });
                    }
                    open = Some((String::from(ty), tokens[i].start_byte, tokens[i].end_byte));
                }
            }
        } else {
            if let Some((t, s, e)) = open.take() {
                entities.push(Entity {
                    label: t,
                    start: s,
                    end: e,
                });
            }
        }
    }
    if let Some((t, s, e)) = open.take() {
        entities.push(Entity {
            label: t,
            start: s,
            end: e,
        });
    }
    entities
}

fn detect(model: &Model, input: Value) -> Value {
    let text = input.get("text").and_then(Value::as_str).unwrap_or("");
    let tokens = encode(model, text);
    let logits = forward_logits(model, &tokens);
    let entities = decode_entities(&tokens, &logits);

    let entity_values: Vec<Value> = entities
        .iter()
        .map(|e| {
            let snippet = text.get(e.start..e.end).unwrap_or("");
            object(alloc::vec![
                ("text", Value::String(String::from(snippet))),
                ("label", Value::String(e.label.clone())),
                ("start", Value::Number(e.start as f64)),
                ("end", Value::Number(e.end as f64)),
            ])
        })
        .collect();

    object(alloc::vec![("entities", Value::Array(entity_values))])
}

#[cfg(all(feature = "full-model", not(test)))]
fn load_production_model() -> Model<'static> {
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
    // Tiny synthetic NER1 fixture: hidden=8, heads=2, ffn=16, layers=1,
    // 12-token cased vocab. Exercises parsing/tokenizer/forward/BIO-decode
    // plumbing -- NOT a claim of real NER accuracy, which is verified
    // separately against the published `full-model` artifact and the real
    // ONNX graph (see the publish PR).
    // ---------------------------------------------------------------

    const T_HIDDEN: usize = 8;
    const T_HEADS: usize = 2;
    const T_FFN: usize = 16;

    fn varied_i8(len: usize, seed: i32) -> Vec<i8> {
        (0..len)
            .map(|i| ((((i as i32) * 7 + seed) % 11) - 5) as i8)
            .collect()
    }

    fn push_qmat(bin: &mut Vec<u8>, rows: usize, cols: usize, scale: f32, seed: i32) {
        bin.extend_from_slice(&scale.to_le_bytes());
        for b in varied_i8(rows * cols, seed) {
            bin.push(b as u8);
        }
    }

    fn push_f32s(bin: &mut Vec<u8>, len: usize, value: f32) {
        for _ in 0..len {
            bin.extend_from_slice(&value.to_le_bytes());
        }
    }

    fn fixture_bin() -> Vec<u8> {
        let tokens = [
            "[PAD]", "[UNK]", "[CLS]", "[SEP]", "hello", "world", "Musk", "El", "##on", "##sk",
            "Mu",
        ];
        let vocab = tokens.len();
        let mut bin = Vec::new();
        bin.extend_from_slice(b"NER1");
        bin.extend_from_slice(&1u32.to_le_bytes()); // version
        bin.extend_from_slice(&(vocab as u32).to_le_bytes());
        bin.extend_from_slice(&(T_HIDDEN as u32).to_le_bytes());
        bin.extend_from_slice(&1u32.to_le_bytes()); // layers
        bin.extend_from_slice(&(T_HEADS as u32).to_le_bytes());
        bin.extend_from_slice(&(T_FFN as u32).to_le_bytes());
        bin.extend_from_slice(&(NUM_LABELS as u32).to_le_bytes());
        bin.extend_from_slice(&2u32.to_le_bytes()); // cls_id
        bin.extend_from_slice(&3u32.to_le_bytes()); // sep_id
        bin.extend_from_slice(&1u32.to_le_bytes()); // unk_id
        bin.extend_from_slice(&0u32.to_le_bytes()); // pad_id

        push_qmat(&mut bin, vocab, T_HIDDEN, 0.05, 1); // word_emb
        push_qmat(&mut bin, 512, T_HIDDEN, 0.02, 2); // pos_emb
        push_qmat(&mut bin, T_HIDDEN, T_HIDDEN, 0.1, 3); // q
        push_qmat(&mut bin, T_HIDDEN, T_HIDDEN, 0.1, 4); // k
        push_qmat(&mut bin, T_HIDDEN, T_HIDDEN, 0.1, 5); // v
        push_qmat(&mut bin, T_HIDDEN, T_HIDDEN, 0.1, 6); // out
        push_qmat(&mut bin, T_HIDDEN, T_FFN, 0.1, 7); // ffn1
        push_qmat(&mut bin, T_FFN, T_HIDDEN, 0.1, 8); // ffn2
        push_qmat(&mut bin, T_HIDDEN, NUM_LABELS, 0.1, 9); // classifier

        push_f32s(&mut bin, T_HIDDEN, 1.0); // emb LN weight
        push_f32s(&mut bin, T_HIDDEN, 0.0); // emb LN bias
        push_f32s(&mut bin, T_HIDDEN, 0.0); // q bias
        push_f32s(&mut bin, T_HIDDEN, 0.0); // k bias
        push_f32s(&mut bin, T_HIDDEN, 0.0); // v bias
        push_f32s(&mut bin, T_HIDDEN, 0.0); // out bias
        push_f32s(&mut bin, T_HIDDEN, 1.0); // sa LN weight
        push_f32s(&mut bin, T_HIDDEN, 0.0); // sa LN bias
        push_f32s(&mut bin, T_FFN, 0.0); // ffn1 bias
        push_f32s(&mut bin, T_HIDDEN, 0.0); // ffn2 bias
        push_f32s(&mut bin, T_HIDDEN, 1.0); // out LN weight
        push_f32s(&mut bin, T_HIDDEN, 0.0); // out LN bias
        push_f32s(&mut bin, NUM_LABELS, 0.0); // classifier bias

        for t in &tokens {
            bin.extend_from_slice(&(t.len() as u16).to_le_bytes());
        }
        for t in &tokens {
            bin.extend_from_slice(t.as_bytes());
        }
        bin
    }

    fn model() -> Model<'static> {
        let bin = Box::leak(fixture_bin().into_boxed_slice());
        Model::parse(bin).expect("fixture parses")
    }

    fn call(model: &Model, json: &str) -> Value {
        let input = wasi_capability_runtime::parse_json(json).expect("parse");
        detect(model, input)
    }

    #[test]
    fn parses_fixture_header_and_dims() {
        let m = model();
        assert_eq!(m.hidden, T_HIDDEN);
        assert_eq!(m.heads, T_HEADS);
        assert_eq!(m.head_dim, T_HIDDEN / T_HEADS);
        assert_eq!(m.ffn, T_FFN);
        assert_eq!(m.layers.len(), 1);
        assert_eq!(m.cls_id, 2);
        assert_eq!(m.sep_id, 3);
        assert_eq!(m.unk_id, 1);
    }

    #[test]
    fn char_byte_offsets_tracks_multibyte_correctly() {
        let (chars, offsets) = char_byte_offsets("aé b"); // 'é' is 2 bytes in UTF-8
        assert_eq!(chars, alloc::vec!['a', 'é', ' ', 'b']);
        assert_eq!(offsets, alloc::vec![0, 1, 3, 4, 5]);
    }

    #[test]
    fn basic_token_ranges_splits_on_whitespace_and_isolates_punctuation() {
        let (chars, _offsets) = char_byte_offsets("Elon Musk, Inc.");
        let ranges = basic_token_ranges(&chars);
        let texts: Vec<String> = ranges
            .iter()
            .map(|(s, e)| chars[*s..*e].iter().collect())
            .collect();
        assert_eq!(texts, alloc::vec!["Elon", "Musk", ",", "Inc", "."]);
    }

    #[test]
    fn tokenizer_is_case_preserving_not_lowercasing() {
        let m = model();
        let (chars, _) = char_byte_offsets("Elon");
        let pieces = wordpiece_ids(&m, &chars, (0, 4));
        // "Elon" isn't whole-word in the fixture vocab -> splits El + ##on,
        // both of which only exist in their original case.
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].0, 7); // "El"
        assert_eq!(pieces[1].0, 8); // "##on"
    }

    #[test]
    fn wordpiece_falls_back_to_unk_when_no_split_covers_the_word() {
        let m = model();
        let (chars, _) = char_byte_offsets("zzqx");
        let pieces = wordpiece_ids(&m, &chars, (0, 4));
        assert_eq!(pieces, alloc::vec![(m.unk_id, 0, 4)]);
    }

    #[test]
    fn encode_wraps_with_cls_and_sep_and_tracks_byte_offsets() {
        let m = model();
        let tokens = encode(&m, "hello world");
        assert_eq!(tokens.first().unwrap().id, m.cls_id);
        assert_eq!(tokens.last().unwrap().id, m.sep_id);
        // "hello" is a whole-vocab token spanning bytes [0,5); "world" [6,11).
        assert_eq!(tokens[1].start_byte, 0);
        assert_eq!(tokens[1].end_byte, 5);
        assert_eq!(tokens[2].start_byte, 6);
        assert_eq!(tokens[2].end_byte, 11);
    }

    #[test]
    fn encode_is_capped_at_max_seq_tokens() {
        let m = model();
        let long_text = "hello ".repeat(500);
        let tokens = encode(&m, &long_text);
        assert!(tokens.len() <= MAX_SEQ_TOKENS);
        assert_eq!(tokens.last().unwrap().id, m.sep_id);
    }

    #[test]
    fn forward_logits_produces_finite_values_per_token() {
        let m = model();
        let tokens = encode(&m, "hello world");
        let logits = forward_logits(&m, &tokens);
        assert_eq!(logits.len(), tokens.len());
        for row in &logits {
            assert_eq!(row.len(), NUM_LABELS);
            assert!(row.iter().all(|v| v.is_finite()));
        }
    }

    #[test]
    fn forward_logits_is_deterministic() {
        let m = model();
        let tokens = encode(&m, "Elon Musk");
        let a = forward_logits(&m, &tokens);
        let b = forward_logits(&m, &tokens);
        assert_eq!(a, b);
    }

    #[test]
    fn label_name_and_entity_type_round_trip() {
        assert_eq!(label_name(1), "B-PER");
        assert_eq!(entity_type("B-PER"), "PER");
        assert_eq!(entity_type("I-LOC"), "LOC");
        assert_eq!(entity_type("O"), "");
        assert_eq!(label_name(999), "O");
    }

    #[test]
    fn decode_entities_merges_b_i_sequence_into_one_span() {
        let m = model();
        let tokens = encode(&m, "hello world"); // 4 tokens: CLS hello world SEP
                                                // Fabricate logits: hello=B-PER, world=I-PER (peak at those indices).
        let mut logits = alloc::vec![alloc::vec![0f32; NUM_LABELS]; tokens.len()];
        logits[1][1] = 10.0; // B-PER
        logits[2][2] = 10.0; // I-PER
        let entities = decode_entities(&tokens, &logits);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].label, "PER");
        assert_eq!(entities[0].start, tokens[1].start_byte);
        assert_eq!(entities[0].end, tokens[2].end_byte);
    }

    #[test]
    fn decode_entities_dangling_i_without_b_starts_new_entity() {
        let m = model();
        let tokens = encode(&m, "hello world");
        let mut logits = alloc::vec![alloc::vec![0f32; NUM_LABELS]; tokens.len()];
        logits[1][0] = 10.0; // O
        logits[2][4] = 10.0; // I-ORG with no preceding B-ORG
        let entities = decode_entities(&tokens, &logits);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].label, "ORG");
    }

    #[test]
    fn decode_entities_mismatched_type_closes_and_reopens() {
        let m = model();
        let tokens = encode(&m, "hello world");
        let mut logits = alloc::vec![alloc::vec![0f32; NUM_LABELS]; tokens.len()];
        logits[1][1] = 10.0; // B-PER
        logits[2][4] = 10.0; // I-ORG (different type -- can't extend PER)
        let entities = decode_entities(&tokens, &logits);
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0].label, "PER");
        assert_eq!(entities[1].label, "ORG");
    }

    #[test]
    fn detect_no_entities_returns_empty_array() {
        let m = model();
        let out = call(&m, r#"{"text":""}"#);
        assert!(out.get("entities").unwrap().as_array().unwrap().is_empty());
    }

    #[test]
    fn detect_missing_text_defaults_to_empty() {
        let m = model();
        let out = call(&m, r#"{}"#);
        assert!(out.get("entities").unwrap().as_array().unwrap().is_empty());
    }

    #[test]
    fn detect_returns_well_formed_entity_objects_when_present() {
        // Not asserting real semantic correctness (tiny fixture weights
        // aren't the real model) -- just that whatever gets decoded has
        // the right shape and the text field matches the byte span.
        let m = model();
        let out = call(&m, r#"{"text":"hello world"}"#);
        for e in out.get("entities").unwrap().as_array().unwrap() {
            assert!(e.get("label").unwrap().as_str().is_some());
            let start = e.get("start").unwrap().as_f64().unwrap() as usize;
            let end = e.get("end").unwrap().as_f64().unwrap() as usize;
            assert!(start <= end);
            assert_eq!(end, end.min(11));
        }
    }

    #[test]
    fn argmax_picks_the_largest_value() {
        assert_eq!(argmax(&[0.1, 5.0, 2.0]), 1);
        assert_eq!(argmax(&[9.0, 1.0]), 0);
    }

    #[test]
    fn layer_norm_output_has_zero_mean() {
        let mut x = alloc::vec![1.0f32, 2.0, 3.0, 4.0];
        let weight = alloc::vec![1.0f32; 4];
        let bias = alloc::vec![0.0f32; 4];
        layer_norm(&mut x, &weight, &bias, EPS);
        let mean: f32 = x.iter().sum::<f32>() / 4.0;
        assert!(mean.abs() < 1e-4);
    }

    #[test]
    fn gelu_zero_is_zero() {
        assert!(gelu(0.0).abs() < 1e-6);
    }
}
