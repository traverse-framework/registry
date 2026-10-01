# Example: complete model rights package

[Spec 026](../../specs/026-model-rights-compliance/spec.md) SC-005 requires a
worked example of a model-backed capability with LICENSE, NOTICE,
attribution, provenance, conversion metadata and data obligations. This
directory is that example. `scripts/ci/tests/test_capability_validation.py`
validates it against every spec 026 gate.

- `capabilities/example/example.detect-things/1.0.0/contract.json` holds the
  contract. See its `ai.models[0]` entry.
- `release-assets/` holds the bytes a publisher would upload to the
  `artifacts/example.detect-things-1.0.0` Release: `LICENSE`, `NOTICE`, and
  the converted weight blob. The `sha256` values in the contract are the
  hashes of these files.
- `capability-src/example-detect-things/model-weights.json` lists the weight
  blob. `derivation.converted_sha256` must match one of its entries.

The upstream model, Release URLs and revision are illustrative and do not
exist. Publisher and consumer obligations are described in
[`docs/model-rights.md`](../../docs/model-rights.md).
