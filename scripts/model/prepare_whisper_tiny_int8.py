#!/usr/bin/env python3
"""Export openai/whisper-tiny to the WHSP1 binary table used by
audio.transcribe-speech (registry#473).

Requires: pip install onnx numpy
License (recorded for registry#473): dual-attested -- the openai/whisper
GitHub repo's own LICENSE file is MIT (verified directly, not a search
summary); the openai/whisper-tiny Hugging Face model card's own metadata
(cardData.license / the API's top-level "license" field) says apache-2.0.
Both are primary sources and both are permissive; logged as-is rather than
picking one, per the registry#473 feasibility research.

Weights are read directly from the model's own published ONNX graphs
(Xenova/whisper-tiny's onnx/encoder_model.onnx + onnx/decoder_model.onnx,
themselves exported from the same openai/whisper-tiny checkpoint) --
resolved via each MatMul *node's own name* (e.g.
"/layers.0/self_attn/q_proj/MatMul"), which retains the full original
module path even though the initializer tensors it references are
anonymized (onnx::MatMul_<n>) -- not by tracing bias->Add->MatMul as
text.detect-entities (registry#465) had to, though that technique would
also work here; this export's node names just make it unnecessary.

All Linear-layer weight matrices in this ONNX export are stored in
MatMul(x, W) "(in, out)" orientation -- confirmed via fc1's rectangular
(384, 1536) shape -- the same orientation already found for
text.detect-entities' DistilBERT-NER ONNX export (registry#465), and the
opposite of HF's native nn.Linear.weight "(out, in)" layout.

Quantization: per-tensor symmetric int8 on every weight *matrix*
(attention q/k/v/out projections and FFN fc1/fc2 for both the encoder and
decoder, plus the decoder's tied embed_tokens table -- by far the largest
single tensor at 51865x384). Convolution weights, all biases, LayerNorm
gamma/beta, and the decoder's learned position-embedding table stay
float32 (small; conv1/conv2 feed the entire encoder so kept at full
precision rather than risking compounded error for a <5%-of-total-size
saving). Verified empirically (scripts producing this binary are mirrored
by capability-src/audio-transcribe-speech's own Python reference in the
publish PR, not re-run here): greedy-decoding a real 11s speech sample
(the openai/whisper repo's own tests/jfk.flac fixture) end to end with
these exact quantized weights produced a semantically identical
transcript to the float32 baseline, differing by exactly one comma token
across the whole 27-30 token output (and, per a close read, the quantized
version's placement is arguably the more accurate rendering of the real
quote) -- unlike registry#460's Silero VAD, where int8 measurably broke a
recurrent LSTM's output, autoregressive *token* decisions proved far more
robust to per-weight quantization noise here, matching (not contradicting)
the stateless-feedforward finding from registry#465's DistilBERT-NER.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import struct
import sys
from pathlib import Path

EXPECTED_SHA256 = "3963e45908f17009ffcb6b16647991f021210b86baea0f558d53c96dcf86460e"

N_MELS = 80
N_MEL_FILTER_BINS = 201  # N_FFT // 2 + 1
N_AUDIO_CTX = 1500
N_AUDIO_STATE = 384
N_AUDIO_HEAD = 6
N_AUDIO_LAYER = 4
N_VOCAB = 51865
BASE_VOCAB = 50257  # multilingual.tiktoken mergeable-rank count
N_TEXT_CTX = 448
N_TEXT_STATE = 384
N_TEXT_HEAD = 6
N_TEXT_LAYER = 4

SOT = 50258
EOT = 50257
LANG_EN = 50259
TASK_TRANSCRIBE = 50359
NO_TIMESTAMPS = 50363


def _load_named_matmul_weights(m):
    from onnx import numpy_helper
    inits = {i.name: numpy_helper.to_array(i) for i in m.graph.initializer}
    out = {}
    for n in m.graph.node:
        if n.op_type != "MatMul":
            continue
        w_name = next((inp for inp in n.input if inp in inits), None)
        if w_name is None:
            continue
        key = n.name.strip("/")
        for prefix in ("model/decoder/", "model/encoder/"):
            if key.startswith(prefix):
                key = key[len(prefix):]
        key = key.rsplit("/MatMul", 1)[0]
        out[key] = inits[w_name]
    return out, inits


def load_encoder_weights(path: Path):
    import onnx
    m = onnx.load(str(path))
    mm, inits = _load_named_matmul_weights(m)
    W = {
        "conv1.weight": inits["conv1.weight"], "conv1.bias": inits["conv1.bias"],
        "conv2.weight": inits["conv2.weight"], "conv2.bias": inits["conv2.bias"],
        "layer_norm.weight": inits["layer_norm.weight"], "layer_norm.bias": inits["layer_norm.bias"],
    }
    for L in range(N_AUDIO_LAYER):
        p = f"layers.{L}"
        for name in ("self_attn/q_proj", "self_attn/k_proj", "self_attn/v_proj", "self_attn/out_proj", "fc1", "fc2"):
            W[f"{p}.{name.replace('/', '.')}.weight"] = mm[f"{p}/{name}"]
        for name in ("self_attn.q_proj.bias", "self_attn.v_proj.bias", "self_attn.out_proj.bias",
                      "self_attn_layer_norm.weight", "self_attn_layer_norm.bias",
                      "fc1.bias", "fc2.bias", "final_layer_norm.weight", "final_layer_norm.bias"):
            W[f"{p}.{name}"] = inits[f"{p}.{name}"]
    return W


def load_decoder_weights(path: Path):
    import onnx
    m = onnx.load(str(path))
    mm, inits = _load_named_matmul_weights(m)
    W = {
        "embed_tokens.weight": inits["model.decoder.embed_tokens.weight"],
        "embed_positions.weight": inits["model.decoder.embed_positions.weight"],
        "layer_norm.weight": inits["model.decoder.layer_norm.weight"],
        "layer_norm.bias": inits["model.decoder.layer_norm.bias"],
    }
    for L in range(N_TEXT_LAYER):
        p = f"layers.{L}"
        for name in ("self_attn/q_proj", "self_attn/k_proj", "self_attn/v_proj", "self_attn/out_proj",
                      "encoder_attn/q_proj", "encoder_attn/k_proj", "encoder_attn/v_proj", "encoder_attn/out_proj",
                      "fc1", "fc2"):
            W[f"{p}.{name.replace('/', '.')}.weight"] = mm[f"{p}/{name}"]
        for name in ("self_attn.q_proj.bias", "self_attn.v_proj.bias", "self_attn.out_proj.bias",
                      "self_attn_layer_norm.weight", "self_attn_layer_norm.bias",
                      "encoder_attn.q_proj.bias", "encoder_attn.v_proj.bias", "encoder_attn.out_proj.bias",
                      "encoder_attn_layer_norm.weight", "encoder_attn_layer_norm.bias",
                      "fc1.bias", "fc2.bias", "final_layer_norm.weight", "final_layer_norm.bias"):
            W[f"{p}.{name}"] = inits[f"model.decoder.{p}.{name}"]
    return W


QUANTIZED_KEYS_ENC = ["layers.{L}.self_attn.q_proj.weight", "layers.{L}.self_attn.k_proj.weight",
                      "layers.{L}.self_attn.v_proj.weight", "layers.{L}.self_attn.out_proj.weight",
                      "layers.{L}.fc1.weight", "layers.{L}.fc2.weight"]
FLOAT_KEYS_ENC_PER_LAYER = ["layers.{L}.self_attn.q_proj.bias", "layers.{L}.self_attn.v_proj.bias",
                            "layers.{L}.self_attn.out_proj.bias",
                            "layers.{L}.self_attn_layer_norm.weight", "layers.{L}.self_attn_layer_norm.bias",
                            "layers.{L}.fc1.bias", "layers.{L}.fc2.bias",
                            "layers.{L}.final_layer_norm.weight", "layers.{L}.final_layer_norm.bias"]

QUANTIZED_KEYS_DEC = ["layers.{L}.self_attn.q_proj.weight", "layers.{L}.self_attn.k_proj.weight",
                      "layers.{L}.self_attn.v_proj.weight", "layers.{L}.self_attn.out_proj.weight",
                      "layers.{L}.encoder_attn.q_proj.weight", "layers.{L}.encoder_attn.k_proj.weight",
                      "layers.{L}.encoder_attn.v_proj.weight", "layers.{L}.encoder_attn.out_proj.weight",
                      "layers.{L}.fc1.weight", "layers.{L}.fc2.weight"]
FLOAT_KEYS_DEC_PER_LAYER = ["layers.{L}.self_attn.q_proj.bias", "layers.{L}.self_attn.v_proj.bias",
                            "layers.{L}.self_attn.out_proj.bias",
                            "layers.{L}.self_attn_layer_norm.weight", "layers.{L}.self_attn_layer_norm.bias",
                            "layers.{L}.encoder_attn.q_proj.bias", "layers.{L}.encoder_attn.v_proj.bias",
                            "layers.{L}.encoder_attn.out_proj.bias",
                            "layers.{L}.encoder_attn_layer_norm.weight", "layers.{L}.encoder_attn_layer_norm.bias",
                            "layers.{L}.fc1.bias", "layers.{L}.fc2.bias",
                            "layers.{L}.final_layer_norm.weight", "layers.{L}.final_layer_norm.bias"]


def _write_f32_tensor(handle, arr):
    import numpy as np
    handle.write(np.asarray(arr, dtype="<f4").tobytes())


def _write_quantized_tensor(handle, arr):
    import numpy as np
    tensor = np.asarray(arr, dtype=np.float32)
    scale = float(np.max(np.abs(tensor))) / 127.0 if tensor.size else 1.0
    if scale == 0.0:
        scale = 1.0
    q = np.clip(np.round(tensor / scale), -127, 127).astype(np.int8)
    handle.write(struct.pack("<f", scale))
    handle.write(q.tobytes())


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, default=Path("capability-src/audio-transcribe-speech/data/whisper-tiny-int8.bin"))
    parser.add_argument("--encoder-onnx", type=Path, help="Path to a locally downloaded Xenova/whisper-tiny onnx/encoder_model.onnx")
    parser.add_argument("--decoder-onnx", type=Path, help="Path to a locally downloaded Xenova/whisper-tiny onnx/decoder_model.onnx")
    parser.add_argument("--mel-filters-npz", type=Path, help="Path to a locally downloaded openai/whisper whisper/assets/mel_filters.npz")
    parser.add_argument("--vocab-tiktoken", type=Path, help="Path to a locally downloaded openai/whisper whisper/assets/multilingual.tiktoken")
    parser.add_argument("--check-only", action="store_true")
    args = parser.parse_args()

    if args.check_only:
        data = args.out.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        if digest != EXPECTED_SHA256:
            print(f"sha256 mismatch: got {digest}, expected {EXPECTED_SHA256}", file=sys.stderr)
            return 1
        print(f"ok {args.out} sha256={digest} bytes={len(data)}")
        return 0

    import numpy as np

    Wenc = load_encoder_weights(args.encoder_onnx)
    Wdec = load_decoder_weights(args.decoder_onnx)
    with np.load(args.mel_filters_npz) as f:
        mel_filters = f["mel_80"].astype(np.float32)
    if mel_filters.shape != (N_MELS, N_MEL_FILTER_BINS):
        print(f"unexpected mel filter shape {mel_filters.shape}", file=sys.stderr)
        return 1

    with args.vocab_tiktoken.open() as f:
        ranks = {base64.b64decode(t): int(r) for t, r in (line.split() for line in f if line)}
    if len(ranks) != BASE_VOCAB:
        print(f"unexpected base vocab size {len(ranks)}, expected {BASE_VOCAB}", file=sys.stderr)
        return 1
    vocab_by_id = [None] * BASE_VOCAB
    for tok_bytes, rank in ranks.items():
        vocab_by_id[rank] = tok_bytes
    if any(v is None for v in vocab_by_id):
        print("gap in tiktoken rank ids", file=sys.stderr)
        return 1

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("wb") as h:
        h.write(b"WHSP1")
        h.write(struct.pack(
            "<IIIIIIIIIIIIII",
            1,  # format version
            N_MELS, N_MEL_FILTER_BINS,
            N_AUDIO_CTX, N_AUDIO_STATE, N_AUDIO_HEAD, N_AUDIO_LAYER,
            N_VOCAB, BASE_VOCAB, N_TEXT_CTX, N_TEXT_STATE, N_TEXT_HEAD, N_TEXT_LAYER,
            0,  # reserved
        ))
        h.write(struct.pack("<IIIII", SOT, EOT, LANG_EN, TASK_TRANSCRIBE, NO_TIMESTAMPS))

        _write_f32_tensor(h, mel_filters)  # (80, 201)

        # Encoder: conv1/conv2 float, then per-layer quantized+float, then final LN.
        _write_f32_tensor(h, Wenc["conv1.weight"]); _write_f32_tensor(h, Wenc["conv1.bias"])
        _write_f32_tensor(h, Wenc["conv2.weight"]); _write_f32_tensor(h, Wenc["conv2.bias"])
        for L in range(N_AUDIO_LAYER):
            for tmpl in QUANTIZED_KEYS_ENC:
                _write_quantized_tensor(h, Wenc[tmpl.format(L=L)])
            for tmpl in FLOAT_KEYS_ENC_PER_LAYER:
                _write_f32_tensor(h, Wenc[tmpl.format(L=L)])
        _write_f32_tensor(h, Wenc["layer_norm.weight"]); _write_f32_tensor(h, Wenc["layer_norm.bias"])

        # Decoder: embed_tokens (quantized) + embed_positions (float), per-layer, final LN.
        _write_quantized_tensor(h, Wdec["embed_tokens.weight"])
        _write_f32_tensor(h, Wdec["embed_positions.weight"])
        for L in range(N_TEXT_LAYER):
            for tmpl in QUANTIZED_KEYS_DEC:
                _write_quantized_tensor(h, Wdec[tmpl.format(L=L)])
            for tmpl in FLOAT_KEYS_DEC_PER_LAYER:
                _write_f32_tensor(h, Wdec[tmpl.format(L=L)])
        _write_f32_tensor(h, Wdec["layer_norm.weight"]); _write_f32_tensor(h, Wdec["layer_norm.bias"])

        # BPE detokenize table: id -> raw bytes, ids 0..BASE_VOCAB-1.
        for tok_bytes in vocab_by_id:
            if len(tok_bytes) > 255:
                print(f"token too long: {tok_bytes!r}", file=sys.stderr)
                return 1
            h.write(struct.pack("<B", len(tok_bytes)))
        for tok_bytes in vocab_by_id:
            h.write(tok_bytes)

    digest = hashlib.sha256(args.out.read_bytes()).hexdigest()
    print(f"wrote {args.out} sha256={digest} bytes={args.out.stat().st_size}")
    if digest != EXPECTED_SHA256:
        print(
            "note: update EXPECTED_SHA256 above (and data/README.md / contract authoring.test_evidence) "
            "with this digest if this run is the one to publish",
            file=sys.stderr,
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
