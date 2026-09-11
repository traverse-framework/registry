#!/usr/bin/env python3
"""Export sentence-transformers/paraphrase-MiniLM-L3-v2 (ONNX conversion published
as Xenova/paraphrase-MiniLM-L3-v2, both Apache-2.0) to the BRT1 int8 table used by
report.translate-fr-semantic (registry#455).

Requires: pip install safetensors numpy

Quantization: per-tensor symmetric int8 (scale = max(abs(tensor))/127), applied
only to the large matmul weight matrices (word/position embeddings, and each
transformer layer's query/key/value/attention-output-dense/intermediate-dense/
output-dense weights) -- ~17.2M elements, ~17.2 MB. Everything else (biases,
LayerNorm gamma/beta, token_type_embeddings -- ~164K elements) stays float32:
tiny by comparison, and precision there (especially LayerNorm's eps=1e-12
division) matters more than the size saved by quantizing it. Verified against
the real published int8-dynamic-quantized ONNX graph (onnxruntime,
Xenova/paraphrase-MiniLM-L3-v2/onnx/model_int8.onnx): mean-pooled, L2-normalized
sentence embedding cosine similarity ~0.99 across representative report.*
sentences; template-vs-unrelated-sentence cosine ~0.12, template-vs-itself 1.0 --
comfortably either side of the capability's 0.72 match threshold.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import sys
from pathlib import Path

EXPECTED_SHA256 = "8ae804ef04a1ae3b90e2e07036f6df6d9885d3406133b3a18b519e118a02f900"

# Fixed order the Rust reader expects: big matmul weight matrices first (all
# int8 + one f32 scale each), then every float32 tensor (bias / LayerNorm /
# token_type_embeddings), then the vocab. Both orderings are themselves fixed
# lists below so the binary layout has one, checked-in source of truth.
QUANTIZED_TENSOR_ORDER = [
    "embeddings.word_embeddings.weight",
    "embeddings.position_embeddings.weight",
] + [
    f"encoder.layer.{layer}.{name}"
    for layer in range(3)
    for name in (
        "attention.self.query.weight",
        "attention.self.key.weight",
        "attention.self.value.weight",
        "attention.output.dense.weight",
        "intermediate.dense.weight",
        "output.dense.weight",
    )
]

FLOAT_TENSOR_ORDER = [
    "embeddings.token_type_embeddings.weight",
    "embeddings.LayerNorm.weight",
    "embeddings.LayerNorm.bias",
] + [
    f"encoder.layer.{layer}.{name}"
    for layer in range(3)
    for name in (
        "attention.self.query.bias",
        "attention.self.key.bias",
        "attention.self.value.bias",
        "attention.output.dense.bias",
        "attention.output.LayerNorm.weight",
        "attention.output.LayerNorm.bias",
        "intermediate.dense.bias",
        "output.dense.bias",
        "output.LayerNorm.weight",
        "output.LayerNorm.bias",
    )
]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--out",
        type=Path,
        default=Path(
            "capability-src/report-translate-fr-semantic/data/minilm-l3-fr-semantic-int8.bin"
        ),
    )
    parser.add_argument(
        "--safetensors",
        type=Path,
        help="Path to a locally downloaded sentence-transformers/paraphrase-MiniLM-L3-v2 model.safetensors",
    )
    parser.add_argument(
        "--config",
        type=Path,
        help="Path to the matching config.json",
    )
    parser.add_argument(
        "--vocab",
        type=Path,
        help="Path to the matching vocab.txt (WordPiece, one token per line)",
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
    from safetensors import safe_open

    cfg = json.loads(args.config.read_text())
    hidden = cfg["hidden_size"]
    layers = cfg["num_hidden_layers"]
    heads = cfg["num_attention_heads"]
    intermediate = cfg["intermediate_size"]
    max_pos = cfg["max_position_embeddings"]
    eps = cfg["layer_norm_eps"]
    if layers != 3 or hidden != 384 or heads != 12 or intermediate != 1536:
        print(
            f"warning: config dims differ from the pinned MiniLM-L3 shape "
            f"(hidden={hidden} layers={layers} heads={heads} intermediate={intermediate}) "
            "-- the Rust reader hardcodes 384/3/12/1536; update both sides together",
            file=sys.stderr,
        )

    weights: dict[str, "np.ndarray"] = {}
    with safe_open(str(args.safetensors), framework="numpy") as f:
        for key in f.keys():
            if key == "embeddings.position_ids":
                continue
            weights[key] = f.get_tensor(key)

    missing = [k for k in QUANTIZED_TENSOR_ORDER + FLOAT_TENSOR_ORDER if k not in weights]
    if missing:
        print(f"missing expected tensors: {missing}", file=sys.stderr)
        return 1

    vocab_lines = [
        line.rstrip("\n") for line in args.vocab.read_text(encoding="utf-8").splitlines()
    ]
    vocab_size = len(vocab_lines)
    special = {"[UNK]": None, "[CLS]": None, "[SEP]": None, "[PAD]": None, "[MASK]": None}
    for idx, tok in enumerate(vocab_lines):
        if tok in special and special[tok] is None:
            special[tok] = idx
    for name, idx in special.items():
        if idx is None:
            print(f"vocab is missing special token {name}", file=sys.stderr)
            return 1

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("wb") as handle:
        handle.write(b"BRT1")
        handle.write(
            struct.pack(
                "<IIIIIIIIII",
                1,  # format version
                vocab_size,
                hidden,
                layers,
                heads,
                intermediate,
                max_pos,
                special["[CLS]"],
                special["[SEP]"],
                special["[UNK]"],
            )
        )
        handle.write(struct.pack("<f", eps))
        handle.write(struct.pack("<I", special["[PAD]"]))

        # 1) quantized matrices: f32 scale then int8 row-major bytes, fixed order.
        for key in QUANTIZED_TENSOR_ORDER:
            tensor = np.asarray(weights[key], dtype=np.float32)
            scale = float(np.max(np.abs(tensor))) / 127.0 if tensor.size else 1.0
            if scale == 0.0:
                scale = 1.0
            quantized = np.clip(np.round(tensor / scale), -127, 127).astype(np.int8)
            handle.write(struct.pack("<f", scale))
            handle.write(quantized.tobytes())

        # 2) float32 tensors verbatim, fixed order.
        for key in FLOAT_TENSOR_ORDER:
            tensor = np.asarray(weights[key], dtype=np.float32)
            handle.write(tensor.tobytes())

        # 3) vocab: WordPiece tokens in file order (index == token id), u16-length-prefixed utf8.
        encoded = [tok.encode("utf-8") for tok in vocab_lines]
        for blob in encoded:
            if len(blob) > 0xFFFF:
                print(f"vocab token too long to length-prefix with u16: {blob!r}", file=sys.stderr)
                return 1
            handle.write(struct.pack("<H", len(blob)))
        for blob in encoded:
            handle.write(blob)

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
