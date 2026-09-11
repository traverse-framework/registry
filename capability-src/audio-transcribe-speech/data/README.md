# Model data for audio.transcribe-speech

Generate the int8 weight table locally (not committed — ~39.9 MB):

```bash
python3 -m venv .venv && .venv/bin/pip install onnx numpy
# download the following (Apache-2.0/MIT, see below):
#  - Xenova/whisper-tiny: onnx/encoder_model.onnx, onnx/decoder_model.onnx
#  - openai/whisper: whisper/assets/mel_filters.npz, whisper/assets/multilingual.tiktoken
.venv/bin/python scripts/model/prepare_whisper_tiny_int8.py \
  --encoder-onnx <path>/encoder_model.onnx \
  --decoder-onnx <path>/decoder_model.onnx \
  --mel-filters-npz <path>/mel_filters.npz \
  --vocab-tiktoken <path>/multilingual.tiktoken
.venv/bin/python scripts/model/prepare_whisper_tiny_int8.py --check-only
```

Pinned sha256: `3963e45908f17009ffcb6b16647991f021210b86baea0f558d53c96dcf86460e`

Also published at:
https://github.com/traverse-framework/registry/releases/download/artifacts/audio.transcribe-speech-1.0.0/whisper-tiny-int8.bin

Release WASM builds require `--features full-model` with this file present.

## Source model

`openai/whisper-tiny` — **license is dual-attested, logged as-is rather than
picking one**: the `openai/whisper` GitHub repo's own `LICENSE` file is MIT
(verified directly against the primary source); the `openai/whisper-tiny`
Hugging Face model card's own metadata (`cardData.license`, and the API's
top-level `license` field) says Apache-2.0. Both are primary sources, both
permissive; no explanation for the discrepancy was found in either source,
so both are recorded rather than guessed at.

Architecture (all verified against openai/whisper's own `audio.py`/
`model.py`/`tokenizer.py`/`decoding.py`, cross-checked against the HF
`config.json`): 80-channel log-mel spectrogram (16kHz, 400-sample FFT,
160-sample hop, Hann window, a fixed 80x201 mel filterbank) -> 2 conv1d
layers (stride 1 then 2, GELU) -> a 4-layer/384-dim/6-head transformer
encoder (sinusoidal position embedding) -> a 4-layer/384-dim/6-head
transformer decoder (learned position embedding, causal self-attention,
cross-attention to the full 1500-frame encoder output) -> greedy,
English-forced, no-timestamps decoding. ~39M parameters.

Weights are read directly from Xenova/whisper-tiny's `onnx/encoder_model.onnx`
and `onnx/decoder_model.onnx` (the same openai/whisper-tiny checkpoint,
re-exported to ONNX), **not** by trusting the ONNX initializer list's raw
names: like `text.detect-entities`' DistilBERT-NER export, this export's
attention/FFN weight *matrices* are anonymous `onnx::MatMul_<n>` tensors.
Unlike that export, though, this one's MatMul **node names** (e.g.
`/layers.0/self_attn/q_proj/MatMul`) retain the full original module path,
so weight resolution here is a direct node-name lookup rather than the
bias->Add->MatMul graph-tracing `text.detect-entities` needed. All Linear
weight matrices are this ONNX export's `MatMul(x, W)` "(in, out)"
orientation — the same convention (and the opposite of HF's native
`nn.Linear.weight` "(out, in)" layout) already found for that DistilBERT-NER
export.

## Quantization

Per-tensor symmetric int8 on every attention/FFN weight matrix (encoder
q/k/v/out/fc1/fc2 x4 layers, decoder self-attn + cross-attn q/k/v/out/fc1/fc2
x4 layers) plus the decoder's tied `embed_tokens` table (51865x384 — by far
the largest tensor, also reused as the output projection). Conv weights,
biases, LayerNorm gamma/beta, and the decoder's learned position embeddings
stay float32 (small; conv1/conv2 feed the entire encoder, kept at full
precision rather than risking compounded error for a small size saving).

Verified empirically end to end (not assumed): greedy-decoded a real 11s
speech sample — `openai/whisper`'s own `tests/jfk.flac` test fixture, the
famous JFK inaugural excerpt ("...ask not what your country can do for
you...") — through the full mel-spectrogram + encoder + autoregressive
decoder pipeline, both with the float32 baseline and with these exact
quantized weights. The quantized transcript matched the float32 baseline's
wording exactly except for one comma token (arguably the *more* accurate
rendering of the real quote). Unlike `audio.detect-speech-segments`'
recurrent Silero VAD (registry#460), where int8 measurably broke the LSTM's
output, autoregressive *token* decisions here proved robust to per-weight
quantization noise — consistent with (not contradicting) `text.detect-entities`'
stateless-feedforward finding for DistilBERT-NER (registry#465).

## Auxiliary assets

- `mel_filters.npz` (`openai/whisper`, MIT): the 80x201 mel filterbank
  matrix, generated once via `librosa.filters.mel(sr=16000, n_fft=400,
  n_mels=80)` and frozen into the repo by upstream — not computed on the
  fly by this capability either, for the same reason (deterministic,
  bit-exact reproducibility against the reference).
- `multilingual.tiktoken` (`openai/whisper`, MIT): the GPT-2-style
  byte-level BPE vocabulary (50,257 base ranks) `openai/whisper-tiny`
  (the multilingual, not `.en`, checkpoint) uses. Only used for
  *detokenizing* (id -> raw UTF-8 bytes) — this capability never needs
  the encode/merge side of BPE, since it only ever consumes model output
  token ids, never tokenizes caller-supplied text.

## Verification

The compiled `wasm32-unknown-unknown --features full-model` artifact was
run under `wasmtime` (CLI) on the same real 11s JFK sample: output matched
the quantized Python reference exactly, byte for byte —
`" And so my fellow Americans ask not what your country can do for you, ask
what you can do for your country."`
