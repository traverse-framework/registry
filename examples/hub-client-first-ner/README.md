# A Hugging Face model as a pinned, signed, client-first capability

[`dslim/distilbert-NER`](https://huggingface.co/dslim/distilbert-NER) is a small named-entity model on the Hugging Face Hub. This registry publishes it as the Traverse capability [`text.detect-entities@1.1.1`](../../capabilities/text/text.detect-entities/1.1.1/contract.json), and this example runs that capability in two places:

- **in a browser tab**
- **in Node on your machine**

Both use the same verified bytes and the same ~150 lines of JavaScript. The text you analyze never leaves the device.

**Try it in your browser:** <https://registry.traverse-framework.com/demos/hub-client-first-ner/>

```text
$ node cli.js "Elon Musk founded SpaceX in California."
✓ Contract text.detect-entities@1.1.1: model dslim/distilbert-NER@dfa2838a (Apache-2.0)
✓ Artifact 65.7 MB, weights embedded; sha256 matches the contract (f11f230260f0…)
✓ Ed25519 signature verifies against the registry key (39d1745be4d5…)
✓ Ran locally in 260 ms (no network after load)

{ "entities": [ { "text": "Elon Musk",  "label": "PER", "start": 0,  "end": 9 },
                { "text": "SpaceX",     "label": "ORG", "start": 18, "end": 24 },
                { "text": "California", "label": "LOC", "start": 28, "end": 38 } ] }
```

## Run it

Requires Node 22 or newer, or any current browser with WebCrypto Ed25519.

```bash
# Local (Node)
node cli.js "Angela Merkel met Emmanuel Macron at the Élysée Palace in Paris."

# Browser: open the hosted copy above, or serve this folder over http
# (ES modules don't load from file://)
npx serve .          # or: python3 -m http.server
```

The first load downloads the 66 MB artifact once from the registry's CORS-enabled mirror. After that, every call runs locally.

## What "pinned and signed" means here

Nothing runs until every link in this chain checks out ([`ner-demo.js`](ner-demo.js)):

| Link | Where it's recorded | Checked by the demo |
|---|---|---|
| Upstream model | `ai.models[0]` in the contract: `dslim/distilbert-NER` at revision [`dfa2838a`](https://huggingface.co/dslim/distilbert-NER/tree/dfa2838a127384aabb82ed7719e16dab84c42a2a), Apache-2.0 | Shown (provenance) |
| Weights | int8 weight table built from that revision by [`prepare_distilbert_ner_int8.py`](../../scripts/model/prepare_distilbert_ner_int8.py), pinned sha256 in [`model-weights.json`](../../capability-src/text-detect-entities/model-weights.json), embedded in the WASM | Covered by the next row |
| Executable | `artifact.digest` in the immutable contract | sha256 of the downloaded bytes must match |
| Publisher | Ed25519 [`signature.json`](../../capabilities/text/text.detect-entities/1.1.1/signature.json) over the artifact bytes | Must verify against the registry key pinned in the demo ([`catalog/signing-key.pub`](../../catalog/signing-key.pub)), not the key the signature file names |

The Hub stays the source of the model. It isn't fetched at runtime and isn't on the critical path. The registry records exactly which revision was packaged, under which license, and signs the result. Versions are immutable: a fix ships as a new version (1.1.1 fixed [#611](https://github.com/traverse-framework/registry/issues/611)), and 1.0.0 still resolves to the same bytes it always did.

## How it runs

The artifact is a plain `wasm32` module that imports three host functions: `fd_read` (the JSON input), `fd_write` (the JSON output) and `proc_exit`. No network, filesystem or clock access is available to it. That is the whole host contract every capability in this registry shares, which is why the same bytes run unchanged in a browser, in Node, or under `wasmtime`:

```bash
echo '{"text":"Sarah Chen from Acme Corp"}' | wasmtime run detect-entities-agent.wasm
```

The model itself is a hand-written `#![no_std]` Rust forward pass (DistilBERT, 6 layers, int8 weights). Source: [`capability-src/text-detect-entities`](../../capability-src/text-detect-entities). Registry CI rebuilds it from that source, with the pinned weights, and runs the contract's examples on the real model before any version can merge.

## What this example is not

- **Not the full Traverse runtime.** It calls the capability directly. The Traverse runtime and embedders add contract-driven workflow planning, trust tiers that require this same signature, and placement decisions. This page shows only the part that is already true everywhere: governed bytes running on the client.
- **No placement or defer story yet.** Deferring a call to edge or cloud needs a second executor, which doesn't exist yet. So this example shows client-side execution only.
- **Not a replacement for Transformers.js or the Hub.** Use those to discover, try and train models. This shows a way to *ship* one as a versioned, signed product unit.
- **Model limits apply.** CoNLL-2003 entity types only (PER, ORG, LOC, MISC), English-centric, and at most 256 WordPiece tokens per call. Output is deterministic within a run; byte-identical output across different host CPUs is not claimed (see the contract's `deterministic-per-call` postcondition).

## Attribution

Model: [`dslim/distilbert-NER`](https://huggingface.co/dslim/distilbert-NER), Apache-2.0, by its copyright holders. Capability code: Apache-2.0, this repository.
