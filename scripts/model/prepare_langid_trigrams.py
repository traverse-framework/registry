#!/usr/bin/env python3
"""Build the LID1 character-trigram frequency table used by
text.detect-language (registry#469), and serve as the Python reference
implementation the compiled wasm is verified against.

Algorithm: Cavnar & Trenkle 1994, "N-Gram-Based Text Categorization"
(classic, public technique -- not a port of any specific codebase, unlike
CLD2 which was investigated and found impractical to port faithfully, see
registry#469). Per-language top-N character-trigram frequency-rank
profiles; classify text by summing, over the text's own top-M trigrams,
the rank-distance to each language's profile (a trigram absent from a
profile costs a fixed max penalty); the language with the LOWEST total
distance wins. Every quantity involved (ranks, distances, penalties) is a
non-negative integer -- there is no floating-point anywhere in scoring, so
this is byte-identical across build/host architectures (risk.determinism_
class: deterministic, not model_derived; matches report.summarize-
semantic's integer-only-hot-path precedent).

Training data: UDHR (Universal Declaration of Human Rights) translations,
public-domain UN source text, accessed via the uiuc-sst/udhr GitHub
corpus (MIT-licensed packaging/tooling; the underlying declaration text
itself is a UN document with no copyright restriction on reproduction).
Downloaded from https://raw.githubusercontent.com/uiuc-sst/udhr/master/text/
Each source file is tab-separated `<line_id>\\t<text>` per line; only the
text column is used.

Usage:
    python3 scripts/model/prepare_langid_trigrams.py \\
        --udhr-dir <dir with <code>.txt files> \\
        --out capability-src/text-detect-language/data/langid-trigrams.bin \\
        --validate
"""

from __future__ import annotations

import argparse
import glob
import hashlib
import os
import random
import struct
import sys
from pathlib import Path

PROFILE_SIZE = 300   # top-N trigrams kept per language in the shipped table
INPUT_TOP_M = 400    # top-M trigrams taken from the text being classified
MIN_INPUT_TRIGRAMS = 5  # below this, report "und" (undetermined) rather than guess
CONFIDENCE_K = 2000   # margin->confidence integer-division constant (see classify())

EXPECTED_SHA256 = "aae9276221f33ddcba1df810129848c254b5281ecb5d5a1704c80317de3ba0a0"


def load_lang_text(path: Path) -> str:
    parts_out = []
    with path.open(encoding="utf-8") as f:
        for line in f:
            parts = line.rstrip("\n").split("\t", 1)
            if len(parts) == 2:
                parts_out.append(parts[1])
    return " ".join(parts_out)


def load_lang_lines(path: Path) -> list[str]:
    lines = []
    with path.open(encoding="utf-8") as f:
        for line in f:
            parts = line.rstrip("\n").split("\t", 1)
            if len(parts) == 2:
                lines.append(parts[1])
    return lines


def trigram_counts(text: str) -> dict[str, int]:
    text = " " + " ".join(text.split()).lower() + " "  # normalize whitespace, pad, lowercase
    counts: dict[str, int] = {}
    chars = list(text)
    for i in range(len(chars) - 2):
        tri = "".join(chars[i:i + 3])
        counts[tri] = counts.get(tri, 0) + 1
    return counts


def ranked_profile(counts: dict[str, int], top_n: int) -> dict[str, int]:
    # Sort by (descending count, ascending trigram bytes) for a fully
    # deterministic order when counts tie -- this exact tie-break must be
    # reproduced bit-for-bit by the Rust port.
    ranked = sorted(counts.items(), key=lambda kv: (-kv[1], kv[0]))[:top_n]
    return {tri: rank for rank, (tri, _cnt) in enumerate(ranked)}


def distance(text_ranks: dict[str, int], lang_profile: dict[str, int], max_penalty: int) -> int:
    total = 0
    for tri, r_text in text_ranks.items():
        r_lang = lang_profile.get(tri)
        total += abs(r_text - r_lang) if r_lang is not None else max_penalty
    return total


def classify(text: str, profiles: dict[str, dict[str, int]], max_penalty: int = PROFILE_SIZE):
    """Reference classifier. Returns (language, confidence_0_100, candidates)
    where candidates is the top-3 (language, distance) pairs ascending by
    distance, ties broken by ascending language code (profiles dict must
    already be in ascending-code order, matching Rust's sorted table)."""
    counts = trigram_counts(text)
    if len(counts) < MIN_INPUT_TRIGRAMS:
        return "und", 0, []
    text_ranks = ranked_profile(counts, INPUT_TOP_M)

    scored = []
    for lang, profile in profiles.items():
        d = distance(text_ranks, profile, max_penalty)
        scored.append((d, lang))
    scored.sort(key=lambda x: (x[0], x[1]))  # distance asc, then code asc (stable tie-break)

    best_dist, best_lang = scored[0]
    second_dist = scored[1][0] if len(scored) > 1 else best_dist + CONFIDENCE_K
    margin = second_dist - best_dist
    confidence = min(100, (margin * 100) // (margin + CONFIDENCE_K))
    candidates = [{"language": lang, "distance": d} for d, lang in scored[:3]]
    return best_lang, confidence, candidates


def encode_table(profiles: dict[str, dict[str, int]]) -> bytes:
    langs = sorted(profiles.keys())
    out = bytearray()
    out += b"LID1"
    out += struct.pack("<III", 1, len(langs), PROFILE_SIZE)
    for lang in langs:
        code_bytes = lang.encode("ascii")
        assert len(code_bytes) == 2, f"expected 2-letter code, got {lang!r}"
        out += code_bytes
        profile = profiles[lang]
        entries = sorted(profile.items(), key=lambda kv: kv[0].encode("utf-8"))
        out += struct.pack("<H", len(entries))
        for tri, rank in entries:
            tri_bytes = tri.encode("utf-8")
            assert 1 <= len(tri_bytes) <= 255, f"trigram encodes to {len(tri_bytes)} bytes: {tri!r}"
            out += struct.pack("<B", len(tri_bytes))
            out += tri_bytes
            out += struct.pack("<H", rank)
    return bytes(out)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--udhr-dir", type=Path, required=True)
    parser.add_argument("--out", type=Path, default=Path("capability-src/text-detect-language/data/langid-trigrams.bin"))
    parser.add_argument("--validate", action="store_true", help="80/20 split accuracy check before writing the full-corpus table")
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

    lang_files = sorted(glob.glob(str(args.udhr_dir / "*.txt")))
    langs = [os.path.splitext(os.path.basename(p))[0] for p in lang_files]
    print(f"languages ({len(langs)}): {langs}")

    if args.validate:
        random.seed(7)
        profiles = {}
        holdout = {}
        for lang in langs:
            lines = load_lang_lines(args.udhr_dir / f"{lang}.txt")
            random.shuffle(lines)
            split = max(1, len(lines) // 5)
            test_lines, train_lines = lines[:split], lines[split:]
            profiles[lang] = ranked_profile(trigram_counts(" ".join(train_lines)), PROFILE_SIZE)
            holdout[lang] = test_lines
        profiles = dict(sorted(profiles.items()))

        correct = total = 0
        errors = []
        for lang, sentences in holdout.items():
            for sent in sentences:
                if len(sent.strip()) < 15:
                    continue
                pred, conf, _cands = classify(sent, profiles)
                total += 1
                if pred == lang:
                    correct += 1
                else:
                    errors.append((lang, pred, conf, sent[:50]))
        print(f"\nheld-out single-sentence accuracy: {correct}/{total} = {100*correct/total:.1f}%")
        for lang, pred, conf, snippet in errors[:15]:
            print(f"  true={lang} pred={pred} conf={conf}  {snippet!r}")
        print()

    # Final shipped table: trained on ALL available text per language (not
    # just the 80% train split above -- the split above is a diagnostic
    # accuracy check, the shipped artifact uses every sentence available).
    full_profiles = {}
    for lang in langs:
        text = load_lang_text(args.udhr_dir / f"{lang}.txt")
        full_profiles[lang] = ranked_profile(trigram_counts(text), PROFILE_SIZE)
    full_profiles = dict(sorted(full_profiles.items()))

    table = encode_table(full_profiles)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(table)
    digest = hashlib.sha256(table).hexdigest()
    print(f"wrote {args.out} sha256={digest} bytes={len(table)}")
    print("note: update EXPECTED_SHA256 above (and data/README.md / contract authoring.test_evidence) with this digest")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
