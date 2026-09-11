# Model data for report.translate-fr-semantic

Generate the int8 weight table locally (not committed — ~17.5 MB):

```bash
python3 -m venv .venv && .venv/bin/pip install safetensors numpy
# download sentence-transformers/paraphrase-MiniLM-L3-v2's model.safetensors,
# config.json, and vocab.txt (Apache-2.0) from Hugging Face, then:
.venv/bin/python scripts/model/prepare_minilm_l3_fr_semantic_int8.py \
  --safetensors <path>/model.safetensors \
  --config <path>/config.json \
  --vocab <path>/vocab.txt
.venv/bin/python scripts/model/prepare_minilm_l3_fr_semantic_int8.py --check-only
```

Pinned sha256: `8ae804ef04a1ae3b90e2e07036f6df6d9885d3406133b3a18b519e118a02f900`

Also published at:
https://github.com/traverse-framework/registry/releases/download/artifacts/report.translate-fr-semantic-1.0.0/minilm-l3-fr-semantic-int8.bin

Release WASM builds require `--features full-model` with this file present.

## Source model

`sentence-transformers/paraphrase-MiniLM-L3-v2` (Apache-2.0), the same weights
published as the ONNX conversion `Xenova/paraphrase-MiniLM-L3-v2` (Apache-2.0,
`base_model: sentence-transformers/paraphrase-MiniLM-L3-v2`) that registry#455
originally named. Weights were sourced from the base `model.safetensors`
checkpoint rather than the ONNX graph because the capability hand-rolls its own
`#![no_std]` transformer forward pass (no `tract-onnx` or any ONNX runtime is
`no_std`-compatible) — the two are numerically equivalent modulo this
capability's own int8 quantization, verified in the publish PR against the
real `Xenova/paraphrase-MiniLM-L3-v2` ONNX graph via `onnxruntime`.

## Quantization

Per-tensor symmetric int8 (`scale = max(abs(tensor)) / 127`), applied only to
the large matmul weight matrices — word/position embeddings and each
transformer layer's query/key/value/attention-output-dense/intermediate-dense/
output-dense weights (~17.2M elements). Biases, LayerNorm gamma/beta, and
`token_type_embeddings` stay float32 (~164K elements, negligible size,
disproportionate precision sensitivity — especially LayerNorm's
`eps=1e-12` division).
