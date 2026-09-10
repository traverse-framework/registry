#!/usr/bin/env python3
"""Export minishlab/potion-base-32M to the M2V1 int8 table used by report.summarize-semantic.

Requires: pip install model2vec numpy
Licenses (recorded for registry#432): potion-base-32M MIT; teacher BAAI/bge-base-en-v1.5 MIT.
"""

from __future__ import annotations

import argparse
import hashlib
import struct
import sys
from pathlib import Path

EXPECTED_SHA256 = "159b2efd704766be0bf2a035c3f49fe62d588d29643c7025f7ce38b0b7584cd1"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--out",
        type=Path,
        default=Path("capability-src/report-summarize-semantic/data/potion-base-32M-int8.bin"),
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

    from model2vec import StaticModel
    import numpy as np

    model = StaticModel.from_pretrained("minishlab/potion-base-32M")
    emb = np.asarray(model.embedding, dtype=np.float32)
    tokens = list(model.tokens)
    unk_id = next(tokens.index(t) for t in ("[UNK]", "<unk>") if t in tokens)
    gscale = float(np.max(np.abs(emb))) / 127.0
    quantized = np.clip(np.round(emb / gscale), -127, 127).astype(np.int8)

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("wb") as handle:
        handle.write(b"M2V1")
        handle.write(
            struct.pack(
                "<IIIII",
                1,
                len(tokens),
                emb.shape[1],
                unk_id,
                struct.unpack("<I", struct.pack("<f", gscale))[0],
            )
        )
        handle.write(quantized.tobytes())
        encoded = [token.encode("utf-8") for token in tokens]
        for blob in encoded:
            handle.write(struct.pack("<H", len(blob)))
        for blob in encoded:
            handle.write(blob)

    digest = hashlib.sha256(args.out.read_bytes()).hexdigest()
    print(f"wrote {args.out} sha256={digest} bytes={args.out.stat().st_size}")
    if digest != EXPECTED_SHA256:
        print(
            "warning: digest differs from pinned EXPECTED_SHA256 — update the constant and contract notes if intentional",
            file=sys.stderr,
        )
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
