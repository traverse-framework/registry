# Trigram table data for text.detect-language

Unlike the other three FR-017 capabilities in this registry (whose multi-MB
neural weight tables are gitignored and hosted on GitHub Releases), this
table is small enough (~52 KB) to commit directly to the repository as
capability source, per registry#469's own gating note ("small tables may be
fine to commit inline as Rust source, unlike the multi-MB neural weight
files"). `langid-trigrams.bin` is a normal, version-controlled file — not
gitignored, no release asset needed.

## Regenerate / verify

```bash
# Fetch the 24 UDHR translation files this table is trained on (see
# "Source data" below for the exact URLs), one <code>.txt per language, into
# some <udhr-dir>, then:
python3 scripts/model/prepare_langid_trigrams.py \
  --udhr-dir <udhr-dir> \
  --out capability-src/text-detect-language/data/langid-trigrams.bin \
  --validate
python3 scripts/model/prepare_langid_trigrams.py \
  --udhr-dir <udhr-dir> \
  --out capability-src/text-detect-language/data/langid-trigrams.bin \
  --check-only
```

Pinned sha256: `aae9276221f33ddcba1df810129848c254b5281ecb5d5a1704c80317de3ba0a0`

## Source data

UDHR (Universal Declaration of Human Rights) translations — public-domain UN
source text — accessed via the MIT-licensed `uiuc-sst/udhr` GitHub corpus
packaging: `https://raw.githubusercontent.com/uiuc-sst/udhr/master/text/<file>.txt`,
one file per language, tab-separated `<line_id>\t<text>` per line (only the
text column is used). 24 languages: ar, bg, cs, da, de, el, en, es, fi, fr,
he, hi, hu, id, it, ja, nl, pl, pt, ro, ru, sv, ta, uk.

## Algorithm

Cavnar & Trenkle 1994, "N-Gram-Based Text Categorization" — a classic,
well-documented public technique, not a port of any specific codebase. This
was a deliberate pivot: the originally proposed model, CLD2 (Compact
Language Detector 2), was investigated against its own primary source
(`CLD2Owners/cld2/internal/`) and found impractical to port faithfully
(~80+MB of hand-packed, undocumented-format C++ n-gram tables across a
dozen+ generated files, multi-stage hashing algorithm, no included offline
table-generation pipeline) — see registry#469 for the full finding.

Per-language top-300 character-trigram frequency-rank profiles are built
from the full UDHR text per language (whitespace-collapsed, lowercased,
space-padded, sliding 3-char window over Unicode codepoints). Classification
sums, over the input text's own top-400 trigrams by frequency, the absolute
rank-distance to each language's profile (a trigram absent from a profile
costs a fixed max penalty of 300); the language with the lowest total
distance wins. An input with fewer than 5 distinct trigrams reports `"und"`
(undetermined) rather than guessing.

## FR-017 / determinism

Every quantity in scoring — ranks, distances, the penalty, the confidence
transform — is a non-negative integer computed by table lookup and integer
subtraction/division. Per decision-log entry 104 Q1, this is **not**
`ai.model_backed: true` (same precedent as `report.summarize-semantic`).
`risk.determinism_class` is `deterministic`: there is no floating-point
arithmetic anywhere in scoring, so output is byte-identical across
build/host architectures.

## Verified accuracy

Python reference (`prepare_langid_trigrams.py --validate`), 80/20
train/held-out split, classifying individual held-out sentences ≥15 chars
(single-sentence input is the realistic hard case): **98.4% (372/378)**
across the 24 trained languages. All recorded errors are close-language-pair
confusions (Czech/Polish, Danish/Swedish, Portuguese/Italian,
Russian/Bulgarian) and all score confidence ≤18/100 — the confidence signal
correctly flags the uncertain cases.

The compiled `wasm32-unknown-unknown` artifact was verified against this
same Python reference via `wasmtime run` on several sentences across
multiple languages plus empty/short-input edge cases: outputs matched
exactly, integer-for-integer (language, confidence, and all three ranked
candidate distances), not just at the top-1 label.
