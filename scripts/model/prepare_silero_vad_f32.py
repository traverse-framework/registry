#!/usr/bin/env python3
"""Export snakers4/silero-vad (16k) to the VAD1 binary table used by
audio.detect-speech-segments (registry#460).

Requires: pip install onnx numpy
License (recorded for registry#460): snakers4/silero-vad is MIT (verified
against the repo's own LICENSE file, not a search-result summary).

Unlike report.summarize-semantic / report.translate-fr-semantic, this model's
weights are NOT quantized. Two reasons, both verified empirically before
choosing: (1) the whole model is already tiny (~1.2 MB float32 -- quantizing
saves nothing worth the risk); (2) per-tensor symmetric int8 on the LSTM
gates measurably breaks output (max|prob diff| ~0.9 against the real ONNX
graph over a 20-chunk recurrent sequence) because LSTM hidden state carries
quantization error forward across every subsequent chunk, unlike a
sentence-embedding mean-pool where per-weight noise averages out. Plain
float32 stays exact (max|diff| ~3e-6, pure rounding noise, over 40 chunks).

Weights are read directly from the published ONNX graph's own initializers
(not a separately-downloaded .safetensors file -- that turned out to be a
numerically different checkpoint of this model, caught by diffing the two
before trusting either; using one file as both the weight source and the
verification oracle removes any cross-file mismatch risk).
"""

from __future__ import annotations

import argparse
import hashlib
import struct
import sys
from pathlib import Path

EXPECTED_SHA256 = "0688f0fadaeba187dc827d90228ea7bb2eda887829e8d9524d0b9535557668fe"

# Fixed order the Rust reader expects. All float32, no quantization (see
# module docstring). ONNX initializer name -> (VAD1 field name, expected shape).
TENSOR_ORDER = [
    ("model.stft.forward_basis_buffer", "stft.weight"),
    ("model.encoder.0.reparam_conv.weight", "conv1.weight"),
    ("model.encoder.0.reparam_conv.bias", "conv1.bias"),
    ("model.encoder.1.reparam_conv.weight", "conv2.weight"),
    ("model.encoder.1.reparam_conv.bias", "conv2.bias"),
    ("model.encoder.2.reparam_conv.weight", "conv3.weight"),
    ("model.encoder.2.reparam_conv.bias", "conv3.bias"),
    ("model.encoder.3.reparam_conv.weight", "conv4.weight"),
    ("model.encoder.3.reparam_conv.bias", "conv4.bias"),
    ("model.decoder.rnn.weight_ih", "lstm.weight_ih"),
    ("model.decoder.rnn.bias_ih", "lstm.bias_ih"),
    ("model.decoder.rnn.weight_hh", "lstm.weight_hh"),
    ("model.decoder.rnn.bias_hh", "lstm.bias_hh"),
    ("model.decoder.decoder.2.weight", "final.weight"),
    ("model.decoder.decoder.2.bias", "final.bias"),
]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("capability-src/audio-detect-speech-segments/data/silero-vad-16k-f32.bin"),
    )
    parser.add_argument(
        "--onnx",
        type=Path,
        help="Path to a locally downloaded snakers4/silero-vad src/silero_vad/data/silero_vad_16k_op15.onnx",
    )
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
    import onnx
    from onnx import numpy_helper

    model = onnx.load(str(args.onnx))
    inits = {i.name: numpy_helper.to_array(i) for i in model.graph.initializer}

    missing = [onnx_name for onnx_name, _ in TENSOR_ORDER if onnx_name not in inits]
    if missing:
        print(f"missing expected ONNX initializers: {missing}", file=sys.stderr)
        return 1

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("wb") as handle:
        handle.write(b"VAD1")
        handle.write(struct.pack("<I", 1))  # format version
        for onnx_name, _field in TENSOR_ORDER:
            tensor = np.asarray(inits[onnx_name], dtype=np.float32)
            handle.write(tensor.tobytes())

    digest = hashlib.sha256(args.out.read_bytes()).hexdigest()
    print(f"wrote {args.out} sha256={digest} bytes={args.out.stat().st_size}")
    if digest != EXPECTED_SHA256:
        print(
            "note: update EXPECTED_SHA256 above (and the contract's authoring.test_evidence / "
            "data/README.md) with this digest if this run is the one to publish",
            file=sys.stderr,
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
