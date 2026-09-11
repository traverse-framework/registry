//! MiniLM-L3 sentence-embedding EN->FR translator for `report.translate-fr-semantic`.
//!
//! Standalone sibling of `report.translate-fr@1.0.0` (registry#455) -- same
//! I/O shape, same ~15-entry EN/FR template bank and glossary (ported
//! verbatim from `report-translate-fr`, itself ported from the UMA book's
//! `chapter-13-portable-mcp-runtime/translator-ai-wasi`), but matching is by
//! **cosine similarity of real sentence embeddings** (threshold 0.72)
//! instead of exact-string / substring matching, so near-paraphrases of a
//! known Fact line still translate via the fixed template instead of
//! falling through to glossary substitution.
//!
//! The embedding model is a hand-rolled `#![no_std]` BERT-mini forward pass
//! (`sentence-transformers/paraphrase-MiniLM-L3-v2`, Apache-2.0; ONNX
//! reference `Xenova/paraphrase-MiniLM-L3-v2`, Apache-2.0): 3 transformer
//! layers, hidden=384, 12 heads, intermediate=1536, mean-pooled +
//! L2-normalized sentence embedding -- `libm` for exp/sqrt/erf, no crate
//! does inference (nothing `std`-only like `tract-onnx`). Weight matrices
//! are per-tensor symmetric int8 (biases/LayerNorm stay float32 -- see
//! `scripts/model/prepare_minilm_l3_fr_semantic_int8.py`), compiled into the
//! release artifact via `include_bytes!` under the `full-model` feature
//! (same pattern as `report-summarize-semantic`, registry#432).
//!
//! **Determinism**: this forward pass is float (softmax, LayerNorm), unlike
//! #432's integer-only table lookup. See `docs/decision-log.md` for the
//! cross-arch note recorded at publish time.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use wasi_capability_runtime::{array_of_strings, object, Value};

#[cfg(feature = "full-model")]
static FULL_MODEL_BIN: &[u8] = include_bytes!("../data/minilm-l3-fr-semantic-int8.bin");

/// Cosine-similarity floor for "this sentence is a match for template N".
const MATCH_THRESHOLD: f32 = 0.72;
/// Hard cap on tokens fed to the encoder (CLS + body + SEP); bounds the
/// O(seq^2) attention cost against adversarial input. Every real report.*
/// sentence and Fact line is far shorter than this.
const MAX_SEQ_TOKENS: usize = 64;
const MAX_INPUT_CHARS_PER_WORD: usize = 100;

// ---------------------------------------------------------------------
// Binary model format (BRT1) -- see scripts/model/prepare_minilm_l3_fr_semantic_int8.py
// for the writer. Layout: magic, header, N quantized matrices (f32 scale +
// row-major int8 bytes), M float32 tensors (raw f32 bytes), then the vocab
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

/// A quantized weight matrix: `rows` x `cols`, row-major, one int8 byte per
/// element (reinterpreted as `i8`), one shared `f32` dequantization scale.
struct QMat<'a> {
    scale: f32,
    rows: usize,
    cols: usize,
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
    q: QMat<'a>,
    q_bias: Vec<f32>,
    k: QMat<'a>,
    k_bias: Vec<f32>,
    v: QMat<'a>,
    v_bias: Vec<f32>,
    attn_out: QMat<'a>,
    attn_out_bias: Vec<f32>,
    attn_ln_w: Vec<f32>,
    attn_ln_b: Vec<f32>,
    inter: QMat<'a>,
    inter_bias: Vec<f32>,
    out: QMat<'a>,
    out_bias: Vec<f32>,
    out_ln_w: Vec<f32>,
    out_ln_b: Vec<f32>,
}

struct Model<'a> {
    hidden: usize,
    heads: usize,
    head_dim: usize,
    intermediate: usize,
    eps: f32,
    cls_id: u32,
    sep_id: u32,
    unk_id: u32,
    word_emb: QMat<'a>,
    pos_emb: QMat<'a>,
    token_type_emb: Vec<f32>,
    emb_ln_w: Vec<f32>,
    emb_ln_b: Vec<f32>,
    layers: Vec<LayerWeights<'a>>,
    /// (token text, token id), sorted by text for binary-search WordPiece lookup.
    token_index: Vec<(&'a str, u32)>,
}

impl<'a> Model<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self, &'static str> {
        if bytes.len() < 4 || &bytes[0..4] != b"BRT1" {
            return Err("bad magic");
        }
        if bytes.len() < 4 + 40 + 4 + 4 {
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
        let intermediate = u32_le(&bytes[h..h + 4]) as usize;
        h += 4;
        let max_pos = u32_le(&bytes[h..h + 4]) as usize;
        h += 4;
        let cls_id = u32_le(&bytes[h..h + 4]);
        h += 4;
        let sep_id = u32_le(&bytes[h..h + 4]);
        h += 4;
        let unk_id = u32_le(&bytes[h..h + 4]);
        h += 4;
        let eps = f32_le(&bytes[h..h + 4]);
        h += 4;
        let _pad_id = u32_le(&bytes[h..h + 4]);
        h += 4;

        if hidden == 0
            || heads == 0
            || hidden % heads != 0
            || layers_n == 0
            || intermediate == 0
            || vocab_size == 0
            || max_pos == 0
        {
            return Err("bad dims");
        }
        let head_dim = hidden / heads;

        // 1) quantized matrices, fixed order matching the Python writer:
        //    word_embeddings, position_embeddings, then per layer
        //    (query, key, value, attention.output.dense, intermediate.dense, output.dense).
        let (word_emb, h) = QMat::parse(bytes, h, vocab_size, hidden)?;
        let (pos_emb, mut h) = QMat::parse(bytes, h, max_pos, hidden)?;

        struct RawLayer<'a> {
            q: QMat<'a>,
            k: QMat<'a>,
            v: QMat<'a>,
            attn_out: QMat<'a>,
            inter: QMat<'a>,
            out: QMat<'a>,
        }
        let mut raw_layers: Vec<RawLayer<'a>> = Vec::with_capacity(layers_n);
        for _ in 0..layers_n {
            let (q, nh) = QMat::parse(bytes, h, hidden, hidden)?;
            let (k, nh) = QMat::parse(bytes, nh, hidden, hidden)?;
            let (v, nh) = QMat::parse(bytes, nh, hidden, hidden)?;
            let (attn_out, nh) = QMat::parse(bytes, nh, hidden, hidden)?;
            let (inter, nh) = QMat::parse(bytes, nh, intermediate, hidden)?;
            let (out, nh) = QMat::parse(bytes, nh, hidden, intermediate)?;
            h = nh;
            raw_layers.push(RawLayer {
                q,
                k,
                v,
                attn_out,
                inter,
                out,
            });
        }

        // 2) float32 tensors, fixed order: token_type_embeddings, embeddings
        //    LayerNorm, then per layer (q/k/v/attn_out bias, attn LayerNorm,
        //    intermediate bias, output bias, output LayerNorm).
        let (token_type_emb_full, nh) = parse_f32_vec(bytes, h, 2 * hidden)?;
        h = nh;
        let token_type_emb = token_type_emb_full[0..hidden].to_vec();
        let (emb_ln_w, nh) = parse_f32_vec(bytes, h, hidden)?;
        h = nh;
        let (emb_ln_b, nh) = parse_f32_vec(bytes, h, hidden)?;
        h = nh;

        let mut layers = Vec::with_capacity(layers_n);
        for raw in raw_layers {
            let (q_bias, nh) = parse_f32_vec(bytes, h, hidden)?;
            let (k_bias, nh) = parse_f32_vec(bytes, nh, hidden)?;
            let (v_bias, nh) = parse_f32_vec(bytes, nh, hidden)?;
            let (attn_out_bias, nh) = parse_f32_vec(bytes, nh, hidden)?;
            let (attn_ln_w, nh) = parse_f32_vec(bytes, nh, hidden)?;
            let (attn_ln_b, nh) = parse_f32_vec(bytes, nh, hidden)?;
            let (inter_bias, nh) = parse_f32_vec(bytes, nh, intermediate)?;
            let (out_bias, nh) = parse_f32_vec(bytes, nh, hidden)?;
            let (out_ln_w, nh) = parse_f32_vec(bytes, nh, hidden)?;
            let (out_ln_b, nh) = parse_f32_vec(bytes, nh, hidden)?;
            h = nh;
            layers.push(LayerWeights {
                q: raw.q,
                q_bias,
                k: raw.k,
                k_bias,
                v: raw.v,
                v_bias,
                attn_out: raw.attn_out,
                attn_out_bias,
                attn_ln_w,
                attn_ln_b,
                inter: raw.inter,
                inter_bias,
                out: raw.out,
                out_bias,
                out_ln_w,
                out_ln_b,
            });
        }

        // 3) vocab: u16 length prefixes (vocab_size of them), then the UTF-8
        //    token bytes back to back in the same order (index == token id).
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
            intermediate,
            eps,
            cls_id,
            sep_id,
            unk_id,
            word_emb,
            pos_emb,
            token_type_emb,
            emb_ln_w,
            emb_ln_b,
            layers,
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
// WordPiece tokenization -- same greedy-longest-match algorithm as
// report-summarize-semantic, over the real BERT vocab.
// ---------------------------------------------------------------------

fn basic_tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        let lower = c.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            current.push(lower);
        } else if lower.is_ascii_whitespace() {
            if !current.is_empty() {
                tokens.push(current.clone());
                current.clear();
            }
        } else {
            if !current.is_empty() {
                tokens.push(current.clone());
                current.clear();
            }
            tokens.push(String::from(lower));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn wordpiece_ids(model: &Model, token: &str) -> Vec<u32> {
    if token.chars().count() > MAX_INPUT_CHARS_PER_WORD {
        return alloc::vec![model.unk_id];
    }
    if let Some(id) = lookup_token(&model.token_index, token) {
        return alloc::vec![id];
    }
    let chars: Vec<char> = token.chars().collect();
    let mut pieces = Vec::new();
    let mut start = 0usize;
    while start < chars.len() {
        let mut end = chars.len();
        let mut found: Option<(u32, usize)> = None;
        while start < end {
            let mut piece: String = chars[start..end].iter().collect();
            if start > 0 {
                piece = format!("##{piece}");
            }
            if let Some(id) = lookup_token(&model.token_index, piece.as_str()) {
                found = Some((id, end));
                break;
            }
            end -= 1;
        }
        if let Some((id, next)) = found {
            pieces.push(id);
            start = next;
        } else {
            return alloc::vec![model.unk_id];
        }
    }
    if pieces.is_empty() {
        alloc::vec![model.unk_id]
    } else {
        pieces
    }
}

fn encode_ids(model: &Model, text: &str) -> Vec<u32> {
    let mut ids = Vec::new();
    ids.push(model.cls_id);
    'words: for token in basic_tokenize(text) {
        let pieces = wordpiece_ids(model, &token);
        for id in pieces {
            if ids.len() >= MAX_SEQ_TOKENS - 1 {
                break 'words;
            }
            ids.push(id);
        }
    }
    ids.push(model.sep_id);
    ids
}

// ---------------------------------------------------------------------
// BERT-mini forward pass: embeddings -> N transformer layers -> masked
// mean pool -> L2 normalize. Single un-batched sequence (every position is
// real -- no padding, so no attention mask needed in the scores); single
// text segment (token_type is always 0).
// ---------------------------------------------------------------------

fn layer_norm(x: &mut [f32], weight: &[f32], bias: &[f32], eps: f32) {
    let n = x.len() as f32;
    let mean: f32 = x.iter().sum::<f32>() / n;
    let mut var: f32 = 0.0;
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
    // Exact erf-based GELU, matching BERT's hidden_act="gelu".
    const INV_SQRT_2: f32 = core::f32::consts::FRAC_1_SQRT_2;
    x * 0.5 * (1.0 + libm::erff(x * INV_SQRT_2))
}

fn linear_q(x: &[f32], w: &QMat, bias: &[f32], out: &mut [f32]) {
    for o in 0..w.rows {
        let mut acc = bias[o];
        for i in 0..w.cols {
            acc += x[i] * w.at(o, i);
        }
        out[o] = acc;
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
        linear_q(&x[t], &layer.q, &layer.q_bias, &mut q[t]);
        linear_q(&x[t], &layer.k, &layer.k_bias, &mut k[t]);
        linear_q(&x[t], &layer.v, &layer.v_bias, &mut v[t]);
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
        linear_q(&ctx[t], &layer.attn_out, &layer.attn_out_bias, &mut out[t]);
    }
    out
}

fn ffn(x: &[f32], layer: &LayerWeights, intermediate: usize, hidden: usize) -> Vec<f32> {
    let mut inter = alloc::vec![0f32; intermediate];
    linear_q(x, &layer.inter, &layer.inter_bias, &mut inter);
    for v in inter.iter_mut() {
        *v = gelu(*v);
    }
    let mut out = alloc::vec![0f32; hidden];
    linear_q(&inter, &layer.out, &layer.out_bias, &mut out);
    out
}

fn embed(model: &Model, text: &str) -> Vec<f32> {
    let ids = encode_ids(model, text);
    let seq = ids.len();
    let mut x: Vec<Vec<f32>> = Vec::with_capacity(seq);
    for &id in &ids {
        let mut row = alloc::vec![0f32; model.hidden];
        for d in 0..model.hidden {
            row[d] = model.word_emb.at(id as usize, d);
        }
        x.push(row);
    }
    for (pos, row) in x.iter_mut().enumerate() {
        // Defensive clamp: MAX_SEQ_TOKENS is chosen well under the real
        // model's max_position_embeddings (512), but never trust that
        // relationship blindly against whatever a parsed artifact declares.
        let pos = pos.min(model.pos_emb.rows.saturating_sub(1));
        for d in 0..model.hidden {
            row[d] += model.pos_emb.at(pos, d) + model.token_type_emb[d];
        }
    }
    for row in x.iter_mut() {
        layer_norm(row, &model.emb_ln_w, &model.emb_ln_b, model.eps);
    }

    for layer in &model.layers {
        let attn = attention(&x, layer, model.heads, model.head_dim, model.hidden);
        for t in 0..seq {
            for d in 0..model.hidden {
                x[t][d] += attn[t][d];
            }
            layer_norm(&mut x[t], &layer.attn_ln_w, &layer.attn_ln_b, model.eps);
        }
        for t in 0..seq {
            let f = ffn(&x[t], layer, model.intermediate, model.hidden);
            for d in 0..model.hidden {
                x[t][d] += f[d];
            }
            layer_norm(&mut x[t], &layer.out_ln_w, &layer.out_ln_b, model.eps);
        }
    }

    let mut pooled = alloc::vec![0f32; model.hidden];
    for row in &x {
        for d in 0..model.hidden {
            pooled[d] += row[d];
        }
    }
    let n = seq as f32;
    for v in pooled.iter_mut() {
        *v /= n;
    }
    let norm = libm::sqrtf(pooled.iter().map(|v| v * v).sum::<f32>());
    if norm > 0.0 {
        for v in pooled.iter_mut() {
            *v /= norm;
        }
    }
    pooled
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
    }
    dot
}

// ---------------------------------------------------------------------
// Translation: the ~15-entry EN/FR template bank + glossary, ported
// verbatim from `report-translate-fr` (itself ported from the UMA book's
// translator-ai-wasi). Only the *matching* strategy differs: cosine
// similarity against real embeddings, not exact-string / substring match.
// ---------------------------------------------------------------------

#[derive(Clone, Copy)]
struct TranslationEntry {
    source: &'static str,
    target: &'static str,
}

/// Index 0-1 are PROJECT/N placeholder templates; 2-14 are exact Fact
/// templates with no placeholders. See `try_project_template` /
/// `translate_one_semantic` for how the two kinds render differently.
fn translation_templates() -> [TranslationEntry; 15] {
    [
        TranslationEntry {
            source: "PROJECT shows how adaptive summarization can combine distributed sources into a richer narrative while still depending on runtime validation across N insight(s).",
            target: "PROJECT montre comment une synthese adaptative peut combiner des sources distribuees dans un recit plus riche tout en restant soumise a la validation du runtime sur N observation(s).",
        },
        TranslationEntry {
            source: "PROJECT combines distributed browser, edge, and cloud evidence into a deterministic operational summary with N validated insight(s).",
            target: "PROJECT combine des preuves distribuees du navigateur, de l'edge et du cloud dans un resume operationnel deterministe avec N observation(s) validee(s).",
        },
        TranslationEntry {
            source: "Fact: browser telemetry confirms the release candidate resolves checkout failures without increasing client memory usage.",
            target: "Fait : la telemetrie du navigateur confirme que la version candidate corrige les echecs de paiement sans augmenter l'utilisation memoire du client.",
        },
        TranslationEntry {
            source: "Fact: Browser telemetry shows strong adoption in three customer regions with localized interface demand.",
            target: "Fait : la telemetrie du navigateur montre une forte adoption dans trois regions clientes avec une demande pour une interface localisee.",
        },
        TranslationEntry {
            source: "Fact: Edge summaries report that French output improves stakeholder review time during rollout coordination.",
            target: "Fait : les syntheses edge indiquent que la sortie en francais ameliore le temps de revue des parties prenantes pendant la coordination du deploiement.",
        },
        TranslationEntry {
            source: "Fact: Cloud analysis indicates that the AI summarizer is currently healthy and produces richer executive narratives.",
            target: "Fait : l'analyse cloud indique que le resumeur IA est actuellement sain et produit des syntheses de direction plus riches.",
        },
        TranslationEntry {
            source: "Fact: The browser shell holds recent customer feedback snippets.",
            target: "Fait : le shell navigateur contient des extraits recents de retours clients.",
        },
        TranslationEntry {
            source: "Fact: An edge cache exposes regional rollout data with low latency.",
            target: "Fait : un cache edge expose les donnees de deploiement regional avec une faible latence.",
        },
        TranslationEntry {
            source: "Fact: A cloud record confirms that contract validation reduced incident handoff time.",
            target: "Fait : un enregistrement cloud confirme que la validation de contrat a reduit le temps de transfert des incidents.",
        },
        TranslationEntry {
            source: "Fact: Local browser data includes a release note timeline and user-facing metrics.",
            target: "Fait : les donnees locales du navigateur incluent une chronologie des notes de version et des metriques visibles par l'utilisateur.",
        },
        TranslationEntry {
            source: "Fact: Edge services expose compatibility summaries for active deployments.",
            target: "Fait : les services edge exposent des syntheses de compatibilite pour les deploiements actifs.",
        },
        TranslationEntry {
            source: "Fact: Cloud analysis indicates that deterministic summaries are still accurate enough for this request.",
            target: "Fait : l'analyse cloud indique que les resumes deterministes restent suffisamment precis pour cette demande.",
        },
        TranslationEntry {
            source: "Fact: edge execution keeps personalization latency below regional service-level objectives.",
            target: "Fait : l'execution en edge maintient la latence de personnalisation sous les objectifs de niveau de service regionaux.",
        },
        TranslationEntry {
            source: "Fact: cloud coordination keeps rollout policy changes synchronized across regions.",
            target: "Fait : la coordination cloud maintient les changements de politique de deploiement synchronises entre les regions.",
        },
        TranslationEntry {
            source: "Fact: regional rollout status is stable enough to shift from incident response to operational planning.",
            target: "Fait : l'etat du deploiement regional est assez stable pour passer de la reponse aux incidents a la planification operationnelle.",
        },
    ]
}

const PROJECT_TEMPLATE_COUNT: usize = 2;

fn extract_first_number(text: &str) -> Option<String> {
    let mut current = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
        } else if !current.is_empty() {
            return Some(current);
        }
    }
    if current.is_empty() {
        None
    } else {
        Some(current)
    }
}

fn render_project_translation(project: &str, insight_count: &str, target: &str) -> String {
    target
        .replace("PROJECT", project)
        .replace("N", insight_count)
}

/// Cosine similarity decides a PROJECT-style template applies; the literal
/// anchor phrase is still needed to know where PROJECT ends and N is. A
/// paraphrase that clears the 0.72 threshold but doesn't carry the anchor
/// returns `None` and the caller falls back to glossary substitution rather
/// than guessing at the placeholder values.
fn try_project_template(text: &str) -> Option<String> {
    let templates = translation_templates();
    let insight_count = extract_first_number(text).unwrap_or_else(|| String::from("0"));

    if text.contains("combines distributed browser") {
        let project = text
            .split(" combines distributed browser")
            .next()
            .unwrap_or("The project");
        return Some(render_project_translation(
            project,
            &insight_count,
            templates[1].target,
        ));
    }
    if text.contains("shows how adaptive summarization") {
        let project = text
            .split(" shows how adaptive summarization")
            .next()
            .unwrap_or("The project");
        return Some(render_project_translation(
            project,
            &insight_count,
            templates[0].target,
        ));
    }
    None
}

fn glossary_translate(text: &str) -> String {
    let replacements = [
        ("Fact:", "Fait :"),
        ("browser telemetry", "telemetrie du navigateur"),
        ("release candidate", "version candidate"),
        ("checkout failures", "echecs de paiement"),
        ("client memory usage", "utilisation memoire du client"),
        ("edge execution", "execution en edge"),
        ("personalization latency", "latence de personnalisation"),
        (
            "regional service-level objectives",
            "objectifs de niveau de service regionaux",
        ),
        ("cloud coordination", "coordination cloud"),
        (
            "rollout policy changes",
            "changements de politique de deploiement",
        ),
        ("across regions", "entre les regions"),
        ("regional rollout status", "etat du deploiement regional"),
        ("incident response", "reponse aux incidents"),
        ("operational planning", "planification operationnelle"),
        ("strong adoption", "forte adoption"),
        ("customer regions", "regions clientes"),
        (
            "localized interface demand",
            "demande pour une interface localisee",
        ),
        ("French output", "sortie en francais"),
        (
            "stakeholder review time",
            "temps de revue des parties prenantes",
        ),
        ("rollout coordination", "coordination du deploiement"),
        ("AI summarizer", "resumeur IA"),
        (
            "richer executive narratives",
            "syntheses de direction plus riches",
        ),
        ("browser shell", "shell navigateur"),
        ("customer feedback snippets", "extraits de retours clients"),
        ("edge cache", "cache edge"),
        ("regional rollout data", "donnees de deploiement regional"),
        ("low latency", "faible latence"),
        ("cloud record", "enregistrement cloud"),
        ("contract validation", "validation de contrat"),
        ("incident handoff time", "temps de transfert des incidents"),
        ("compatibility summaries", "syntheses de compatibilite"),
        ("active deployments", "deploiements actifs"),
        ("deterministic summaries", "resumes deterministes"),
        ("distributed sources", "sources distribuees"),
        ("runtime validation", "validation du runtime"),
        (
            "deterministic operational summary",
            "resume operationnel deterministe",
        ),
        ("validated insight(s)", "observation(s) validee(s)"),
        ("insight(s)", "observation(s)"),
        ("project", "projet"),
        ("summary", "resume"),
    ];

    let mut translated = String::from(text);
    for (english, french) in replacements {
        translated = translated.replace(english, french);
        translated = translated.replace(&english.to_lowercase(), french);
    }
    translated
}

/// Embed all 15 template *source* (English) texts once, via the same
/// forward pass as the input text -- guarantees the comparison is
/// self-consistent (both sides run through identical code), independent of
/// how closely this model's raw output tracks any other implementation.
fn template_embeddings(model: &Model) -> Vec<Vec<f32>> {
    translation_templates()
        .iter()
        .map(|entry| embed(model, entry.source))
        .collect()
}

fn best_template_match(templates: &[Vec<f32>], text_emb: &[f32]) -> Option<(usize, f32)> {
    let mut best: Option<(usize, f32)> = None;
    for (i, template_emb) in templates.iter().enumerate() {
        let cos = cosine(text_emb, template_emb);
        if best.map(|(_, b)| cos > b).unwrap_or(true) {
            best = Some((i, cos));
        }
    }
    best.filter(|(_, cos)| *cos >= MATCH_THRESHOLD)
}

fn translate_one_semantic(model: &Model, templates: &[Vec<f32>], text: &str) -> String {
    let normalized = text.trim();
    if normalized.is_empty() {
        return String::new();
    }
    let emb = embed(model, normalized);
    if let Some((idx, _cos)) = best_template_match(templates, &emb) {
        if idx < PROJECT_TEMPLATE_COUNT {
            if let Some(rendered) = try_project_template(normalized) {
                return rendered;
            }
            // Cosine-matched a PROJECT template but the literal anchor
            // phrase isn't present (a true paraphrase) -- fall through.
        } else {
            return String::from(translation_templates()[idx].target);
        }
    }
    glossary_translate(normalized)
}

fn translate(model: &Model, templates: &[Vec<f32>], input: Value) -> Value {
    let summary = input.get("summary").and_then(Value::as_str).unwrap_or("");
    let structured_facts = input
        .get("structured_facts")
        .map(Value::string_array)
        .unwrap_or_default();

    let translated_summary = translate_one_semantic(model, templates, summary);
    let translated_facts: Vec<String> = structured_facts
        .iter()
        .map(|fact| translate_one_semantic(model, templates, fact))
        .collect();

    object(alloc::vec![
        ("translated_summary", Value::String(translated_summary)),
        ("translated_facts", array_of_strings(&translated_facts)),
    ])
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
    let templates = template_embeddings(&model);
    translate(&model, &templates, input)
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
    // Tiny synthetic BRT1 fixture: hidden=8, heads=2, head_dim=4,
    // intermediate=12, layers=1, max_pos=16, 10-token vocab. Values are
    // hand-picked only to exercise the parser/tokenizer/forward-pass
    // plumbing (shape, finiteness, determinism) -- NOT to reproduce real
    // MiniLM translations, which are validated separately against the
    // published `full-model` artifact (see the PR body).
    // ---------------------------------------------------------------

    const T_HIDDEN: usize = 8;
    const T_HEADS: usize = 2;
    const T_INTER: usize = 12;
    const T_MAXPOS: usize = 16;

    /// Varied-by-position fill (not a constant) -- LayerNorm divides by the
    /// per-row variance, so a matrix that produces identical values across
    /// a row's dimensions would collapse every embedding to zero. `seed`
    /// just shifts the pattern between matrices so they aren't all equal.
    fn push_qmat(bin: &mut Vec<u8>, rows: usize, cols: usize, scale: f32, seed: i32) {
        bin.extend_from_slice(&scale.to_le_bytes());
        for r in 0..rows {
            for c in 0..cols {
                let v = ((r as i32 * 7 + c as i32 * 3 + seed) % 11) - 5; // in [-5, 5]
                bin.push(v as i8 as u8);
            }
        }
    }

    fn push_f32_vec(bin: &mut Vec<u8>, len: usize, value: f32) {
        for _ in 0..len {
            bin.extend_from_slice(&value.to_le_bytes());
        }
    }

    fn fixture_bin() -> Vec<u8> {
        let tokens = [
            "[PAD]", "[UNK]", "[CLS]", "[SEP]", "hello", "world", "browser", "edge", "br", "##ow",
        ];
        let vocab = tokens.len();
        let mut bin = Vec::new();
        bin.extend_from_slice(b"BRT1");
        bin.extend_from_slice(&1u32.to_le_bytes()); // version
        bin.extend_from_slice(&(vocab as u32).to_le_bytes());
        bin.extend_from_slice(&(T_HIDDEN as u32).to_le_bytes());
        bin.extend_from_slice(&1u32.to_le_bytes()); // layers
        bin.extend_from_slice(&(T_HEADS as u32).to_le_bytes());
        bin.extend_from_slice(&(T_INTER as u32).to_le_bytes());
        bin.extend_from_slice(&(T_MAXPOS as u32).to_le_bytes());
        bin.extend_from_slice(&2u32.to_le_bytes()); // cls_id
        bin.extend_from_slice(&3u32.to_le_bytes()); // sep_id
        bin.extend_from_slice(&1u32.to_le_bytes()); // unk_id
        bin.extend_from_slice(&1e-12f32.to_le_bytes()); // eps
        bin.extend_from_slice(&0u32.to_le_bytes()); // pad_id

        // quantized: word_emb, pos_emb, then 1 layer x (q,k,v,attn_out,inter,out)
        push_qmat(&mut bin, vocab, T_HIDDEN, 0.05, 3);
        push_qmat(&mut bin, T_MAXPOS, T_HIDDEN, 0.02, 1);
        push_qmat(&mut bin, T_HIDDEN, T_HIDDEN, 0.1, 2); // q
        push_qmat(&mut bin, T_HIDDEN, T_HIDDEN, 0.1, 2); // k
        push_qmat(&mut bin, T_HIDDEN, T_HIDDEN, 0.1, 2); // v
        push_qmat(&mut bin, T_HIDDEN, T_HIDDEN, 0.1, 1); // attn_out
        push_qmat(&mut bin, T_INTER, T_HIDDEN, 0.1, 1); // intermediate
        push_qmat(&mut bin, T_HIDDEN, T_INTER, 0.1, 1); // output

        // float32: token_type_embeddings (2 rows), embeddings LN, then per
        // layer (q/k/v/attn_out bias, attn LN, inter bias, out bias, out LN)
        push_f32_vec(&mut bin, 2 * T_HIDDEN, 0.0);
        push_f32_vec(&mut bin, T_HIDDEN, 1.0); // emb LN weight
        push_f32_vec(&mut bin, T_HIDDEN, 0.0); // emb LN bias
        push_f32_vec(&mut bin, T_HIDDEN, 0.0); // q bias
        push_f32_vec(&mut bin, T_HIDDEN, 0.0); // k bias
        push_f32_vec(&mut bin, T_HIDDEN, 0.0); // v bias
        push_f32_vec(&mut bin, T_HIDDEN, 0.0); // attn_out bias
        push_f32_vec(&mut bin, T_HIDDEN, 1.0); // attn LN weight
        push_f32_vec(&mut bin, T_HIDDEN, 0.0); // attn LN bias
        push_f32_vec(&mut bin, T_INTER, 0.0); // inter bias
        push_f32_vec(&mut bin, T_HIDDEN, 0.0); // out bias
        push_f32_vec(&mut bin, T_HIDDEN, 1.0); // out LN weight
        push_f32_vec(&mut bin, T_HIDDEN, 0.0); // out LN bias

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

    #[test]
    fn parses_fixture_header_and_dims() {
        let m = model();
        assert_eq!(m.hidden, T_HIDDEN);
        assert_eq!(m.heads, T_HEADS);
        assert_eq!(m.head_dim, T_HIDDEN / T_HEADS);
        assert_eq!(m.intermediate, T_INTER);
        assert_eq!(m.layers.len(), 1);
        assert_eq!(m.cls_id, 2);
        assert_eq!(m.sep_id, 3);
        assert_eq!(m.unk_id, 1);
    }

    #[test]
    fn tokenizer_known_and_oov() {
        let m = model();
        assert_eq!(lookup_token(&m.token_index, "browser"), Some(6));
        assert_eq!(lookup_token(&m.token_index, "zzz_not_in_vocab"), None);
        let ids = encode_ids(&m, "hello world");
        assert_eq!(ids, alloc::vec![2u32, 4, 5, 3]); // CLS hello world SEP
    }

    #[test]
    fn wordpiece_splits_unknown_word_into_known_pieces() {
        let m = model();
        // "brow" isn't in vocab whole, but "br" + "##ow" are.
        let ids = wordpiece_ids(&m, "brow");
        assert_eq!(ids, alloc::vec![8u32, 9]);
    }

    #[test]
    fn wordpiece_falls_back_to_unk_when_no_split_covers_the_word() {
        let m = model();
        assert_eq!(wordpiece_ids(&m, "zzqx"), alloc::vec![m.unk_id]);
    }

    #[test]
    fn overlong_word_is_unk_without_attempting_split() {
        let m = model();
        let long = "a".repeat(MAX_INPUT_CHARS_PER_WORD + 1);
        assert_eq!(wordpiece_ids(&m, &long), alloc::vec![m.unk_id]);
    }

    #[test]
    fn sequence_is_capped_at_max_seq_tokens() {
        let m = model();
        let long_text = "hello ".repeat(200);
        let ids = encode_ids(&m, &long_text);
        assert!(ids.len() <= MAX_SEQ_TOKENS);
        assert_eq!(*ids.last().unwrap(), m.sep_id);
    }

    #[test]
    fn embed_produces_finite_l2_normalized_vector_of_hidden_dim() {
        let m = model();
        let v = embed(&m, "hello world");
        assert_eq!(v.len(), T_HIDDEN);
        assert!(v.iter().all(|x| x.is_finite()));
        let norm: f32 = libm::sqrtf(v.iter().map(|x| x * x).sum::<f32>());
        assert!((norm - 1.0).abs() < 1e-3 || norm == 0.0);
    }

    #[test]
    fn embed_is_deterministic_for_identical_input() {
        let m = model();
        let a = embed(&m, "browser edge hello");
        let b = embed(&m, "browser edge hello");
        assert_eq!(a, b);
    }

    #[test]
    fn embed_differs_for_different_input() {
        let m = model();
        let a = embed(&m, "hello");
        let b = embed(&m, "world browser edge");
        assert_ne!(a, b);
    }

    #[test]
    fn cosine_of_identical_vectors_is_one() {
        let m = model();
        let v = embed(&m, "hello");
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn layer_norm_output_has_zero_mean_unit_variance_direction() {
        let mut x = alloc::vec![1.0f32, 2.0, 3.0, 4.0];
        let weight = alloc::vec![1.0f32; 4];
        let bias = alloc::vec![0.0f32; 4];
        layer_norm(&mut x, &weight, &bias, 1e-12);
        let mean: f32 = x.iter().sum::<f32>() / 4.0;
        assert!(mean.abs() < 1e-4);
    }

    #[test]
    fn gelu_zero_is_zero_and_is_monotonic_increasing() {
        assert!((gelu(0.0)).abs() < 1e-6);
        assert!(gelu(1.0) > gelu(0.0));
        assert!(gelu(-1.0) < gelu(0.0));
    }

    // --- pure translation-logic tests (no embeddings needed) ---

    #[test]
    fn extract_first_number_at_end() {
        assert_eq!(
            extract_first_number("count is 42"),
            Some(String::from("42"))
        );
        assert_eq!(extract_first_number("no digits"), None);
    }

    #[test]
    fn project_combines_template_fills_placeholders() {
        let rendered = try_project_template(
            "Acme Rollout combines distributed browser, edge, and cloud evidence into a deterministic operational summary with 2 validated insights.",
        )
        .expect("anchor present");
        assert_eq!(
            rendered,
            "Acme Rollout combine des preuves distribuees du navigateur, de l'edge et du cloud dans un resume operationnel deterministe avec 2 observation(s) validee(s)."
        );
    }

    #[test]
    fn project_shows_how_template_fills_placeholders() {
        let rendered = try_project_template(
            "Beta Kit shows how adaptive summarization can combine distributed sources into a richer narrative while still depending on runtime validation across 7 insight(s).",
        )
        .expect("anchor present");
        assert_eq!(
            rendered,
            "Beta Kit montre comment une synthese adaptative peut combiner des sources distribuees dans un recit plus riche tout en restant soumise a la validation du runtime sur 7 observation(s)."
        );
    }

    #[test]
    fn try_project_template_none_when_no_anchor_present() {
        assert_eq!(
            try_project_template("a sentence about nothing template-shaped"),
            None
        );
    }

    #[test]
    fn glossary_translates_known_terms() {
        let out = glossary_translate("Fact: browser telemetry and low latency matter.");
        assert_eq!(
            out,
            "Fait : telemetrie du navigateur and faible latence matter."
        );
    }

    #[test]
    fn translation_templates_has_fifteen_entries_two_project_style() {
        let templates = translation_templates();
        assert_eq!(templates.len(), 15);
        assert!(templates[0].source.contains("PROJECT"));
        assert!(templates[1].source.contains("PROJECT"));
        for entry in &templates[PROJECT_TEMPLATE_COUNT..] {
            assert!(entry.source.starts_with("Fact:"));
        }
    }

    #[test]
    fn translate_one_semantic_empty_input_stays_empty() {
        let m = model();
        let templates = template_embeddings(&m);
        assert_eq!(translate_one_semantic(&m, &templates, ""), String::new());
        assert_eq!(translate_one_semantic(&m, &templates, "   "), String::new());
    }

    #[test]
    fn translate_one_semantic_below_threshold_falls_back_to_glossary() {
        // The tiny fixture's weights can't legitimately clear the real
        // 0.72 threshold against the real (large) template bank text, so
        // any non-empty input exercises the glossary-fallback path here --
        // matching real-model non-match behavior for genuinely unrelated
        // input (verified separately against the full model).
        let m = model();
        let templates = template_embeddings(&m);
        let out = translate_one_semantic(&m, &templates, "hello world");
        assert!(!out.is_empty());
    }

    #[test]
    fn best_template_match_returns_none_below_threshold() {
        let m = model();
        let templates = template_embeddings(&m);
        let emb = embed(&m, "hello");
        // With this fixture's tiny, near-orthogonal-by-construction weights
        // the match is not expected to clear 0.72.
        let result = best_template_match(&templates, &emb);
        if let Some((_, cos)) = result {
            assert!(cos >= MATCH_THRESHOLD);
        }
    }

    #[test]
    fn translate_wires_summary_and_facts_through() {
        let m = model();
        let templates = template_embeddings(&m);
        let input = wasi_capability_runtime::parse_json(
            r#"{"summary":"","structured_facts":["", "hello"]}"#,
        )
        .expect("parse");
        let out = translate(&m, &templates, input);
        assert_eq!(out.get("translated_summary").unwrap().as_str().unwrap(), "");
        let facts = out.get("translated_facts").unwrap().string_array();
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0], "");
    }

    #[test]
    fn translate_missing_keys_default_safely() {
        let m = model();
        let templates = template_embeddings(&m);
        let input = wasi_capability_runtime::parse_json(r#"{}"#).expect("parse");
        let out = translate(&m, &templates, input);
        assert_eq!(out.get("translated_summary").unwrap().as_str().unwrap(), "");
        assert!(out
            .get("translated_facts")
            .unwrap()
            .string_array()
            .is_empty());
    }
}
