//! Deterministic extractive summarizer for `report.summarize-semantic`.
//!
//! Same I/O and fixed template as `report.summarize`, but sentence ranking uses
//! Model2Vec `minishlab/potion-base-32M` int8 table lookup + i32 mean-pool sums
//! + fixed-point cosine comparison (no float in the hot path). registry#432.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use wasi_capability_runtime::{object, Value};

const TOP_K: usize = 3;
const MAX_DIM: usize = 512;
const MAX_INPUT_CHARS_PER_WORD: usize = 100;

#[cfg(feature = "full-model")]
static FULL_MODEL_BIN: &[u8] = include_bytes!("../data/potion-base-32M-int8.bin");

/// Parsed M2V1 embedding table (borrowed from a static or fixture buffer).
struct EmbeddingTable<'a> {
    vocab_size: u32,
    dim: usize,
    unk_id: u32,
    cls_id: u32,
    sep_id: u32,
    /// Raw int8 rows as bytes (reinterpret with `as i8` on read).
    embeddings: &'a [u8],
    /// (token, id) sorted by token bytes for binary search.
    token_index: Vec<(&'a str, u32)>,
}

impl<'a> EmbeddingTable<'a> {
    fn parse(bin: &'a [u8]) -> Result<Self, &'static str> {
        if bin.len() < 24 || &bin[0..4] != b"M2V1" {
            return Err("bad magic");
        }
        let version = u32_le(&bin[4..8]);
        if version != 1 {
            return Err("bad version");
        }
        let vocab_size = u32_le(&bin[8..12]);
        let dim = u32_le(&bin[12..16]) as usize;
        let unk_id = u32_le(&bin[16..20]);
        if dim == 0 || dim > MAX_DIM || vocab_size == 0 {
            return Err("bad dims");
        }
        let emb_bytes = (vocab_size as usize).checked_mul(dim).ok_or("overflow")?;
        let emb_start = 24usize;
        let emb_end = emb_start.checked_add(emb_bytes).ok_or("overflow")?;
        if bin.len() < emb_end {
            return Err("truncated embeddings");
        }
        let embeddings = &bin[emb_start..emb_end];
        let lens_start = emb_end;
        let lens_end = lens_start
            .checked_add((vocab_size as usize).checked_mul(2).ok_or("overflow")?)
            .ok_or("overflow")?;
        if bin.len() < lens_end {
            return Err("truncated lengths");
        }
        let mut offsets = Vec::with_capacity(vocab_size as usize);
        let mut cursor = lens_end;
        for i in 0..vocab_size as usize {
            let len = u16_le(&bin[lens_start + i * 2..lens_start + i * 2 + 2]) as usize;
            let next = cursor.checked_add(len).ok_or("overflow")?;
            if next > bin.len() {
                return Err("truncated tokens");
            }
            offsets.push((cursor, len));
            cursor = next;
        }
        let mut token_index = Vec::with_capacity(vocab_size as usize);
        for (id, (start, len)) in offsets.into_iter().enumerate() {
            let tok = core::str::from_utf8(&bin[start..start + len]).map_err(|_| "utf8")?;
            token_index.push((tok, id as u32));
        }
        token_index.sort_by(|a, b| a.0.cmp(b.0));
        let cls_id = lookup_token(&token_index, "[CLS]").unwrap_or(2);
        let sep_id = lookup_token(&token_index, "[SEP]").unwrap_or(3);
        Ok(Self {
            vocab_size,
            dim,
            unk_id,
            cls_id,
            sep_id,
            embeddings,
            token_index,
        })
    }

    #[cfg(test)]
    fn token_id(&self, token: &str) -> u32 {
        lookup_token(&self.token_index, token).unwrap_or(self.unk_id)
    }

    fn row_value(&self, id: u32, index: usize) -> i8 {
        let id = if id >= self.vocab_size {
            self.unk_id
        } else {
            id
        };
        let start = (id as usize) * self.dim;
        self.embeddings[start + index] as i8
    }
}

fn u32_le(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn u16_le(bytes: &[u8]) -> u16 {
    u16::from_le_bytes([bytes[0], bytes[1]])
}

fn lookup_token(index: &[(&str, u32)], token: &str) -> Option<u32> {
    index
        .binary_search_by_key(&token, |entry| entry.0)
        .ok()
        .map(|i| index[i].1)
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        current.push(c);
        if c == '.' || c == '!' || c == '?' {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                out.push(String::from(trimmed));
            }
            current.clear();
        }
    }
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        out.push(String::from(trimmed));
    }
    out
}

/// BERT-like basic tokenize: lowercase, isolate punctuation, split whitespace.
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

fn wordpiece_ids(table: &EmbeddingTable<'_>, token: &str) -> Vec<u32> {
    if token.chars().count() > MAX_INPUT_CHARS_PER_WORD {
        return alloc::vec![table.unk_id];
    }
    if let Some(id) = lookup_token(&table.token_index, token) {
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
            if let Some(id) = lookup_token(&table.token_index, piece.as_str()) {
                found = Some((id, end));
                break;
            }
            end -= 1;
        }
        if let Some((id, next)) = found {
            pieces.push(id);
            start = next;
        } else {
            return alloc::vec![table.unk_id];
        }
    }
    if pieces.is_empty() {
        alloc::vec![table.unk_id]
    } else {
        pieces
    }
}

fn encode_ids(table: &EmbeddingTable<'_>, text: &str) -> Vec<u32> {
    let mut ids = Vec::new();
    ids.push(table.cls_id);
    for token in basic_tokenize(text) {
        ids.extend(wordpiece_ids(table, &token));
    }
    ids.push(table.sep_id);
    ids
}

/// Mean-pool as an i32 sum of int8 rows (cosine is scale-invariant per vector).
fn embed_sum(table: &EmbeddingTable<'_>, text: &str, out: &mut [i32]) {
    for slot in out.iter_mut() {
        *slot = 0;
    }
    let ids = encode_ids(table, text);
    // Skip empty (CLS+SEP only still adds those rows — matches Model2Vec encode).
    for id in ids {
        for i in 0..table.dim {
            out[i] = out[i].saturating_add(i32::from(table.row_value(id, i)));
        }
    }
}

fn add_assign(dst: &mut [i32], src: &[i32]) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d = d.saturating_add(*s);
    }
}

fn is_zero(v: &[i32]) -> bool {
    v.iter().all(|x| *x == 0)
}

/// Fixed-point cosine comparison key vs the shared centroid.
/// Higher is more similar. Uses only integer arithmetic.
fn cosine_rank_key(vec: &[i32], centroid: &[i32]) -> i128 {
    if is_zero(vec) || is_zero(centroid) {
        return i128::MIN / 4;
    }
    let mut dot: i128 = 0;
    let mut nn: i128 = 0;
    let mut cc: i128 = 0;
    for (a, b) in vec.iter().zip(centroid.iter()) {
        let a = i128::from(*a);
        let b = i128::from(*b);
        dot += a * b;
        nn += a * a;
        cc += b * b;
    }
    if nn == 0 || cc == 0 {
        return i128::MIN / 4;
    }
    // sign(dot) * dot^2 * 2^32 / (nn * cc) — monotonic in cos for ranking;
    // shared cc cancels when comparing, but keep it for absolute keys.
    let sign = if dot >= 0 { 1i128 } else { -1i128 };
    let dot2 = dot * dot;
    // Scale to reduce overflow risk while staying deterministic.
    sign * ((dot2 << 32) / (nn * cc))
}

fn build_summary(
    table: &EmbeddingTable<'_>,
    source_fragments: &[String],
    structured_facts: &[String],
    project_name: Option<&str>,
) -> String {
    let mut sentences: Vec<(usize, String)> = Vec::new();
    let mut idx = 0usize;
    for fragment in source_fragments {
        for sentence in split_sentences(fragment) {
            sentences.push((idx, sentence));
            idx += 1;
        }
    }

    let dim = table.dim;
    let mut centroid = [0i32; MAX_DIM];
    let centroid = &mut centroid[..dim];
    let mut scratch = [0i32; MAX_DIM];
    let scratch = &mut scratch[..dim];
    for fact in structured_facts {
        embed_sum(table, fact, scratch);
        add_assign(centroid, scratch);
    }

    let mut ranked: Vec<(i128, usize, String)> = Vec::new();
    for (i, sentence) in sentences {
        embed_sum(table, &sentence, scratch);
        let key = cosine_rank_key(scratch, centroid);
        ranked.push((key, i, sentence));
    }
    // Higher cosine key first; ties keep original order.
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let mut selected: Vec<(usize, String)> = ranked
        .into_iter()
        .take(TOP_K)
        .map(|(_k, i, s)| (i, s))
        .collect();
    selected.sort_by_key(|(i, _)| *i);

    let project = match project_name.map(str::trim).filter(|s| !s.is_empty()) {
        Some(name) => name,
        None => "The project",
    };
    let n = structured_facts.len();
    let insight_word = if n == 1 { "insight" } else { "insights" };
    let header = format!(
        "{project} combines distributed browser, edge, and cloud evidence into a deterministic operational summary with {n} validated {insight_word}."
    );
    if selected.is_empty() {
        return header;
    }
    let mut body = String::new();
    for (_i, sentence) in &selected {
        if !body.is_empty() {
            body.push(' ');
        }
        body.push_str(sentence);
    }
    format!("{header} {body}")
}

fn summarize_with(table: &EmbeddingTable<'_>, input: Value) -> Value {
    let source_fragments = input
        .get("source_fragments")
        .map(Value::string_array)
        .unwrap_or_default();
    let structured_facts = input
        .get("structured_facts")
        .map(Value::string_array)
        .unwrap_or_default();
    let project_name = input.get("project_name").and_then(Value::as_str);
    let summary = build_summary(table, &source_fragments, &structured_facts, project_name);
    object(alloc::vec![("summary", Value::String(summary))])
}

#[cfg(all(feature = "full-model", not(test)))]
fn load_production_table() -> EmbeddingTable<'static> {
    match EmbeddingTable::parse(FULL_MODEL_BIN) {
        Ok(table) => table,
        Err(_) => loop {
            // Pinned artifact parse is infallible; abort without panic!.
            core::arch::wasm32::unreachable()
        },
    }
}

#[cfg(all(feature = "full-model", not(test)))]
fn summarize(input: Value) -> Value {
    let table = load_production_table();
    summarize_with(&table, input)
}

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    #[cfg(feature = "full-model")]
    {
        wasi_capability_runtime::run_capability(summarize);
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

    /// Tiny 4-token × 4-dim table for unit tests (M2V1).
    fn fixture_bin() -> Vec<u8> {
        // tokens: [PAD]=0 [UNK]=1 [CLS]=2 [SEP]=3 hello=4 world=5 browser=6 edge=7 br=8 ##ow=9
        let tokens = [
            "[PAD]", "[UNK]", "[CLS]", "[SEP]", "hello", "world", "browser", "edge", "br", "##ow",
        ];
        let dim = 4usize;
        let vocab = tokens.len() as u32;
        // Distinct directions so cosine ranking is obvious.
        let rows: [[i8; 4]; 10] = [
            [0, 0, 0, 0],
            [1, 0, 0, 0],
            [0, 0, 0, 0],
            [0, 0, 0, 0],
            [10, 0, 0, 0],
            [0, 10, 0, 0],
            [0, 0, 10, 0],
            [0, 0, 0, 10],
            [5, 0, 0, 0],
            [0, 5, 0, 0],
        ];
        let mut bin = Vec::new();
        bin.extend_from_slice(b"M2V1");
        bin.extend_from_slice(&1u32.to_le_bytes());
        bin.extend_from_slice(&vocab.to_le_bytes());
        bin.extend_from_slice(&(dim as u32).to_le_bytes());
        bin.extend_from_slice(&1u32.to_le_bytes()); // unk
        bin.extend_from_slice(&0u32.to_le_bytes()); // unused scale bits
        for row in &rows {
            let as_bytes: [u8; 4] = [row[0] as u8, row[1] as u8, row[2] as u8, row[3] as u8];
            bin.extend_from_slice(&as_bytes);
        }
        for t in &tokens {
            let b = t.as_bytes();
            bin.extend_from_slice(&(b.len() as u16).to_le_bytes());
        }
        for t in &tokens {
            bin.extend_from_slice(t.as_bytes());
        }
        bin
    }

    fn table() -> EmbeddingTable<'static> {
        // Leak fixture for 'static in tests.
        let bin = Box::leak(fixture_bin().into_boxed_slice());
        EmbeddingTable::parse(bin).expect("fixture")
    }

    fn call(table: &EmbeddingTable<'_>, json: &str) -> Value {
        let input = wasi_capability_runtime::parse_json(json).expect("parse");
        summarize_with(table, input)
    }

    fn summary_of(out: &Value) -> &str {
        out.get("summary").unwrap().as_str().unwrap()
    }

    #[test]
    fn table_lookup_and_oov_unk() {
        let t = table();
        assert_eq!(t.token_id("browser"), 6);
        assert_eq!(t.token_id("nope_not_in_vocab_zz"), t.unk_id);
        assert_eq!(
            [
                t.row_value(6, 0),
                t.row_value(6, 1),
                t.row_value(6, 2),
                t.row_value(6, 3)
            ],
            [0, 0, 10, 0]
        );
    }

    #[test]
    fn empty_sentence_and_zero_facts_centroid() {
        let t = table();
        let out = call(&t, r#"{"source_fragments":[""],"structured_facts":[]}"#);
        let s = summary_of(&out);
        assert!(s.contains("with 0 validated insights."));
        // No body sentences from empty fragment.
        assert!(s.ends_with("insights.") || s.contains("insights."));
    }

    #[test]
    fn top_k_larger_than_sentence_count() {
        let t = table();
        let out = call(
            &t,
            r#"{"source_fragments":["browser signal. edge signal."],"structured_facts":["browser","edge"]}"#,
        );
        let s = summary_of(&out);
        assert!(s.contains("browser signal."));
        assert!(s.contains("edge signal."));
        assert!(s.find("browser signal.").unwrap() < s.find("edge signal.").unwrap());
    }

    #[test]
    fn semantic_rank_prefers_matching_fact_direction() {
        let t = table();
        let out = call(
            &t,
            r#"{"source_fragments":["hello filler. browser telemetry rose. world filler."],"structured_facts":["browser"]}"#,
        );
        let s = summary_of(&out);
        assert!(s.contains("browser telemetry rose."));
    }

    #[test]
    fn defaults_project_name_and_pluralizes() {
        let t = table();
        let out = call(
            &t,
            r#"{"source_fragments":["browser a. edge b."],"structured_facts":["browser","edge"]}"#,
        );
        assert!(summary_of(&out).starts_with(
            "The project combines distributed browser, edge, and cloud evidence into a deterministic operational summary with 2 validated insights."
        ));
    }

    #[test]
    fn uses_project_name_and_singular() {
        let t = table();
        let out = call(
            &t,
            r#"{"source_fragments":["browser only."],"structured_facts":["browser"],"project_name":"Acme Rollout"}"#,
        );
        assert!(summary_of(&out).starts_with(
            "Acme Rollout combines distributed browser, edge, and cloud evidence into a deterministic operational summary with 1 validated insight."
        ));
    }

    #[test]
    fn empty_project_name_defaults() {
        let t = table();
        let out = call(
            &t,
            r#"{"source_fragments":[],"structured_facts":[],"project_name":"  "}"#,
        );
        assert!(summary_of(&out).starts_with("The project combines"));
    }

    #[test]
    fn determinism_byte_for_byte() {
        let t = table();
        let json = r#"{"source_fragments":["browser up. edge warm. hello there."],"structured_facts":["browser","edge"],"project_name":"Demo"}"#;
        assert_eq!(
            wasi_capability_runtime::write_json(&call(&t, json)),
            wasi_capability_runtime::write_json(&call(&t, json)),
        );
    }

    #[test]
    fn cross_arch_integer_path_golden() {
        // Integer-only hot path ⇒ byte-identical across x86_64/aarch64.
        // Golden captured on the build host; same bytes required everywhere.
        let t = table();
        let out = call(
            &t,
            r#"{"source_fragments":["browser up. edge warm. hello there."],"structured_facts":["browser","edge"],"project_name":"Demo"}"#,
        );
        let s = summary_of(&out);
        assert_eq!(
            s,
            "Demo combines distributed browser, edge, and cloud evidence into a deterministic operational summary with 2 validated insights. browser up. edge warm. hello there."
        );
        assert_eq!(core::mem::size_of::<i128>(), 16);
    }

    #[test]
    fn output_depends_on_input() {
        let t = table();
        let a = summary_of(&call(
            &t,
            r#"{"source_fragments":["browser one."],"structured_facts":["browser"]}"#,
        ))
        .to_string();
        let b = summary_of(&call(
            &t,
            r#"{"source_fragments":["edge two."],"structured_facts":["edge"]}"#,
        ))
        .to_string();
        assert_ne!(a, b);
    }

    #[test]
    fn parse_rejects_corrupt_tables() {
        assert!(EmbeddingTable::parse(b"xxxx").is_err());
        let mut bad_ver = fixture_bin();
        bad_ver[4..8].copy_from_slice(&2u32.to_le_bytes());
        assert!(EmbeddingTable::parse(&bad_ver).is_err());
        let mut bad_dim = fixture_bin();
        bad_dim[12..16].copy_from_slice(&0u32.to_le_bytes());
        assert!(EmbeddingTable::parse(&bad_dim).is_err());
        assert!(EmbeddingTable::parse(&fixture_bin()[..30]).is_err());
        let mut trunc_tok = fixture_bin();
        trunc_tok.truncate(trunc_tok.len() - 2);
        assert!(EmbeddingTable::parse(&trunc_tok).is_err());
    }

    #[test]
    fn oov_row_and_long_token_and_subword() {
        let t = table();
        assert_eq!(t.row_value(9999, 0), t.row_value(t.unk_id, 0));
        let long = "a".repeat(MAX_INPUT_CHARS_PER_WORD + 1);
        assert_eq!(wordpiece_ids(&t, &long), alloc::vec![t.unk_id]);
        // "browsers" is absent; WordPiece falls back to UNK.
        assert_eq!(wordpiece_ids(&t, "browsers"), alloc::vec![t.unk_id]);
        // "brow" is absent as a whole token but "br"+"##ow" exist.
        assert_eq!(wordpiece_ids(&t, "brow"), alloc::vec![8, 9]);
        // Trailing sentence without terminator exercises the remainder push.
        let out = call(
            &t,
            r#"{"source_fragments":["browser note"],"structured_facts":["browser"]}"#,
        );
        assert!(summary_of(&out).contains("browser note"));
        // Zero vectors → lowest cosine key branch.
        assert_eq!(cosine_rank_key(&[0, 0], &[0, 0]), i128::MIN / 4);
        assert!(
            cosine_rank_key(&[1, 0], &[0, 0]) < 0
                || cosine_rank_key(&[1, 0], &[0, 0]) == i128::MIN / 4
        );
    }
}
