#!/usr/bin/env python3
"""Export dslim/distilbert-NER to the NER1 int8 binary table used by
text.detect-entities (registry#465).

Requires: pip install onnx numpy
License (recorded for registry#465): dslim/distilbert-NER is Apache-2.0,
verified against the model's own Hugging Face API metadata
(cardData.license == "apache-2.0", tag "license:apache-2.0" -- primary
source fields, not a search-result summary).

Weights are read directly from the model's own published ONNX graph
(onnx/model.onnx in the same HF repo) rather than any transformers/PyTorch
checkpoint -- and, critically, NOT by trusting the initializer list's raw
names: DistilBERT's per-layer weight *matrices* (q_lin/k_lin/v_lin/out_lin/
ffn.lin1/ffn.lin2) are exported as anonymous `onnx::MatMul_<n>` tensors, only
their *biases* are named. Each layer's weight tensors were resolved by
tracing the graph forward from each named `.bias` initializer through its
consuming `Add` node to the `MatMul` that feeds it (see the discovery script
in the publish PR / docs/decision-log.md) -- not by assuming initializer
list order matches logical layer order (registry#460 already found a
same-repo file can silently be a different checkpoint; this is the ONNX
analogue: don't trust unlabeled structure either).

Quantization: per-tensor symmetric int8 on every weight *matrix* (word/
position embeddings + each layer's q/k/v/out_lin + ffn.lin1/lin2 + the
final classifier, ~65.1M elements); biases and LayerNorm gamma/beta stay
float32 (~61K elements, negligible size). Verified against the real ONNX
graph: 0/65 label (argmax) mismatches across 5 varied test sentences before
and after quantization -- unlike the recurrent Silero VAD model
(registry#460), this is a plain feedforward transformer call with no
state carried between invocations, so quantization noise doesn't compound.
"""

from __future__ import annotations

import argparse
import hashlib
import struct
import sys
from pathlib import Path

EXPECTED_SHA256 = "12891d6c53ff7808d2dd6edfe78235cf7ad8d17a8ebdbd5d14f31a9b6998e120"

HIDDEN = 768
HEADS = 12
FFN = 3072
LAYERS = 6
NUM_LABELS = 9

# Quantized (int8 + f32 scale), fixed order.
QUANTIZED_TENSOR_ORDER = ["word_emb", "pos_emb"] + [
    f"L{layer}.{suf}" for layer in range(LAYERS) for suf in ("q_w", "k_w", "v_w", "out_w", "ffn1_w", "ffn2_w")
] + ["classifier_w"]

# Float32 verbatim, fixed order.
FLOAT_TENSOR_ORDER = ["emb_ln_w", "emb_ln_b"] + [
    f"L{layer}.{suf}"
    for layer in range(LAYERS)
    for suf in ("q_b", "k_b", "v_b", "out_b", "sa_ln_w", "sa_ln_b", "ffn1_b", "ffn2_b", "out_ln_w", "out_ln_b")
] + ["classifier_b"]


def _resolve_weights(onnx_path: Path):
    import onnx
    from onnx import numpy_helper

    model = onnx.load(str(onnx_path))
    graph = model.graph
    inits = {i.name: numpy_helper.to_array(i) for i in graph.initializer}

    producer = {}
    for node in graph.node:
        for out in node.output:
            producer[out] = node

    def find_add_using(bias_name: str):
        for node in graph.node:
            if node.op_type == "Add" and bias_name in node.input:
                return node
        raise KeyError(f"no Add node consumes {bias_name}")

    def weight_for_bias(bias_name: str):
        add_node = find_add_using(bias_name)
        other = [x for x in add_node.input if x != bias_name][0]
        prod = producer[other]
        if prod.op_type != "MatMul":
            raise ValueError(f"expected MatMul feeding {bias_name}, got {prod.op_type}")
        w_name = [x for x in prod.input if x in inits][0]
        return inits[w_name]

    W = {
        "word_emb": inits["distilbert.embeddings.word_embeddings.weight"],
        "pos_emb": inits["distilbert.embeddings.position_embeddings.weight"],
        "emb_ln_w": inits["distilbert.embeddings.LayerNorm.weight"],
        "emb_ln_b": inits["distilbert.embeddings.LayerNorm.bias"],
        "classifier_b": inits["classifier.bias"],
    }
    W["classifier_w"] = weight_for_bias("classifier.bias")
    for layer in range(LAYERS):
        p = f"distilbert.transformer.layer.{layer}."
        for suf, bias_key in (
            ("q", "attention.q_lin.bias"),
            ("k", "attention.k_lin.bias"),
            ("v", "attention.v_lin.bias"),
            ("out", "attention.out_lin.bias"),
            ("ffn1", "ffn.lin1.bias"),
            ("ffn2", "ffn.lin2.bias"),
        ):
            W[f"L{layer}.{suf}_w"] = weight_for_bias(p + bias_key)
            W[f"L{layer}.{suf}_b"] = inits[p + bias_key]
        W[f"L{layer}.sa_ln_w"] = inits[p + "sa_layer_norm.weight"]
        W[f"L{layer}.sa_ln_b"] = inits[p + "sa_layer_norm.bias"]
        W[f"L{layer}.out_ln_w"] = inits[p + "output_layer_norm.weight"]
        W[f"L{layer}.out_ln_b"] = inits[p + "output_layer_norm.bias"]
    return W


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("capability-src/text-detect-entities/data/distilbert-ner-int8.bin"),
    )
    parser.add_argument("--onnx", type=Path, help="Path to a locally downloaded dslim/distilbert-NER onnx/model.onnx")
    parser.add_argument("--vocab", type=Path, help="Path to the matching vocab.txt (WordPiece, cased)")
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

    W = _resolve_weights(args.onnx)
    missing = [k for k in QUANTIZED_TENSOR_ORDER + FLOAT_TENSOR_ORDER if k not in W]
    if missing:
        print(f"missing resolved tensors: {missing}", file=sys.stderr)
        return 1

    vocab_lines = [line.rstrip("\n") for line in args.vocab.read_text(encoding="utf-8").splitlines()]
    vocab_size = len(vocab_lines)
    if vocab_size != W["word_emb"].shape[0]:
        print(f"vocab size {vocab_size} != word_emb rows {W['word_emb'].shape[0]}", file=sys.stderr)
        return 1
    special = {"[UNK]": None, "[CLS]": None, "[SEP]": None, "[PAD]": None}
    for idx, tok in enumerate(vocab_lines):
        if tok in special and special[tok] is None:
            special[tok] = idx
    for name, idx in special.items():
        if idx is None:
            print(f"vocab is missing special token {name}", file=sys.stderr)
            return 1

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("wb") as handle:
        handle.write(b"NER1")
        handle.write(
            struct.pack(
                "<IIIIIIIIII",
                1,  # format version
                vocab_size,
                HIDDEN,
                LAYERS,
                HEADS,
                FFN,
                NUM_LABELS,
                special["[CLS]"],
                special["[SEP]"],
                special["[UNK]"],
            )
        )
        handle.write(struct.pack("<I", special["[PAD]"]))

        for key in QUANTIZED_TENSOR_ORDER:
            tensor = np.asarray(W[key], dtype=np.float32)
            scale = float(np.max(np.abs(tensor))) / 127.0 if tensor.size else 1.0
            if scale == 0.0:
                scale = 1.0
            quantized = np.clip(np.round(tensor / scale), -127, 127).astype(np.int8)
            handle.write(struct.pack("<f", scale))
            handle.write(quantized.tobytes())

        for key in FLOAT_TENSOR_ORDER:
            tensor = np.asarray(W[key], dtype=np.float32)
            handle.write(tensor.tobytes())

        encoded = [tok.encode("utf-8") for tok in vocab_lines]
        for blob in encoded:
            if len(blob) > 0xFFFF:
                print(f"vocab token too long: {blob!r}", file=sys.stderr)
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
