# Model data for text.detect-entities

Generate the int8 weight table locally (not committed — ~65.6 MB):

```bash
python3 -m venv .venv && .venv/bin/pip install onnx numpy
# download dslim/distilbert-NER's onnx/model.onnx (Apache-2.0) and vocab.txt, then:
.venv/bin/python scripts/model/prepare_distilbert_ner_int8.py \
  --onnx <path>/model.onnx \
  --vocab <path>/vocab.txt
.venv/bin/python scripts/model/prepare_distilbert_ner_int8.py --check-only
```

Pinned sha256: `12891d6c53ff7808d2dd6edfe78235cf7ad8d17a8ebdbd5d14f31a9b6998e120`

Also published at:
https://github.com/traverse-framework/registry/releases/download/artifacts/text.detect-entities-1.0.0/distilbert-ner-int8.bin

Release WASM builds require `--features full-model` with this file present.

## Source model

`dslim/distilbert-NER` (Apache-2.0 — verified directly against the model's own
Hugging Face API metadata, `cardData.license == "apache-2.0"` and the
`license:apache-2.0` tag, not a search-result summary). DistilBERT fine-tuned
on CoNLL-2003: 6 layers, hidden=768, 12 heads, intermediate=3072, cased
28996-token WordPiece vocab, 9-label BIO token classification.

Weights are read directly from the model's own `onnx/model.onnx` (in the same
HF repo), **not** by trusting the ONNX initializer list's raw names or order:
DistilBERT's per-layer weight *matrices* (`q_lin`/`k_lin`/`v_lin`/`out_lin`/
`ffn.lin1`/`ffn.lin2`) are exported as anonymous `onnx::MatMul_<n>` tensors —
only their biases are named. Each layer's weight tensor was resolved by
tracing the graph forward from its named `.bias` initializer, through the
`Add` node that consumes it, to the `MatMul` that feeds that `Add` (see
`_resolve_weights` in the prep script). This ONNX file's `MatMul(x, W)`
weights are stored `(in, out)` — the opposite orientation from the HF
`nn.Linear.weight` `(out, in)` convention `report-translate-fr-semantic`
reads from safetensors — the Rust reader's `QMat` accounts for this.

## Quantization

Per-tensor symmetric int8 on every weight matrix (word/position embeddings,
each layer's `q`/`k`/`v`/`out`/`ffn1`/`ffn2` weights, and the final
classifier — ~65.1M elements); biases and LayerNorm gamma/beta stay float32
(~61K elements, negligible size). Verified against the real ONNX graph:
0/65 label (argmax) mismatches across 5 varied test sentences before/after
quantization. Unlike the recurrent Silero VAD (registry#460), this is a
stateless feedforward call with nothing carried between invocations, so
quantization noise doesn't compound the way it does across a chunked
recurrent sequence.
