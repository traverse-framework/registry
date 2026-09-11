# Model data for audio.detect-speech-segments

Generate the float32 weight table locally (not committed — ~1.2 MB):

```bash
python3 -m venv .venv && .venv/bin/pip install onnx numpy
# download snakers4/silero-vad's src/silero_vad/data/silero_vad_16k_op15.onnx (MIT), then:
.venv/bin/python scripts/model/prepare_silero_vad_f32.py --onnx <path>/silero_vad_16k_op15.onnx
.venv/bin/python scripts/model/prepare_silero_vad_f32.py --check-only
```

Pinned sha256: `0688f0fadaeba187dc827d90228ea7bb2eda887829e8d9524d0b9535557668fe`

Also published at:
https://github.com/traverse-framework/registry/releases/download/artifacts/audio.detect-speech-segments-1.0.0/silero-vad-16k-f32.bin

Release WASM builds require `--features full-model` with this file present.

## Source model

`snakers4/silero-vad` (MIT license — verified directly against the repository's own
`LICENSE` file, not a search-result summary). This capability's weights are read
directly from the model's own published ONNX graph
(`silero_vad_16k_op15.onnx`) initializers — not from the repo's separately
distributed `silero_vad_16k.safetensors`, which turned out to be a
**numerically different checkpoint** (weight tensors differ by up to ~4.0 in
magnitude between the two files, discovered by diffing them directly before
trusting either). Using one file as both the weight source and the
verification oracle removes any cross-file mismatch risk.

## Why no quantization (unlike report.summarize-semantic / report.translate-fr-semantic)

Tested empirically before deciding:

- The whole model is already tiny (~1.2 MB float32) — quantizing saves little.
- Per-tensor symmetric int8 on the LSTM gates measurably breaks output:
  `max|probability diff|` ≈ 0.9 against the real ONNX graph over a 20-chunk
  recurrent sequence, because the LSTM hidden state carries quantization
  error forward across every subsequent chunk (unlike a sentence-embedding
  mean-pool, where per-weight noise averages out across many dimensions).
- Plain float32 stays exact: `max|diff|` ≈ 3e-6 (pure float rounding noise)
  over 40 chunks.

So the shipped artifact carries all 15 weight tensors as raw float32, no
quantization step, no per-tensor scale.
