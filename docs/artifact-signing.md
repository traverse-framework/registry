# Artifact signing runbook

Implements `specs/007-artifact-hosting`'s amendment (registry#331 / registry#333,
`docs/decision-log.md` entries 74–76). Every non-deprecated, WASM-backed
capability published here needs an Ed25519 `signature.json` sibling so
`traverse-runtime` will execute it under its `PublishedGoverned` trust tier —
digest verification alone proves integrity, not authenticity.

## What is already built (registry#334)

- **`scripts/ci/sign_artifacts.py`** — downloads a capability's `artifact.url`,
  signs the exact bytes with Ed25519, writes
  `capabilities/<ns>/<id>/<version>/signature.json` (never touches the immutable
  `contract.json`), and refreshes `catalog/signing-key.pub`.
  - `--since-merge` — sign only contracts added by the current push to `main`
    (`$BEFORE_SHA..$AFTER_SHA`). This is what CI runs.
  - `--all` — sign every non-deprecated, artifact-bearing version that has no
    signature yet. This is the registry#335 backfill, and a self-healing net.
  - `--dry-run` — report targets, write nothing, need no key.
- **`.github/workflows/ci.yml` → `sign-artifacts` job** — `push` to `main` only,
  `continue-on-error: true`, commits the siblings back with `[skip ci]`.
- **`scripts/ci/capability_validation.py`** — validates the shape of any
  `signature.json` that exists and forbids modifying one once written.

The job is **inert** until the two owner-only steps below are done: with no
signing key configured it exits 0 after printing a "key not provisioned" notice
(same posture as `deploy-catalog` before GitHub Pages was enabled).

## Owner setup (one time)

### 1. Generate the keypair

On a trusted local machine (Python with `cryptography` installed):

```bash
python3 - <<'EOF'
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization
k = Ed25519PrivateKey.generate()
seed = k.private_bytes(serialization.Encoding.Raw, serialization.PrivateFormat.Raw,
                       serialization.NoEncryption())
pub = k.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
print("SECRET (store as the Actions secret):", seed.hex())
print("PUBLIC (published automatically by CI):", pub.hex())
EOF
```

Keep the secret hex offline (a password manager). The public hex needs no
protection — CI writes it to `catalog/signing-key.pub` on the first signing run.

### 2. Add the Actions secret

```bash
gh secret set ARTIFACT_SIGNING_ED25519_SECRET_KEY \
  --repo traverse-framework/registry \
  --body '<64-hex-char secret from step 1>'
```

### 3. Let Actions push to `main`

The `sign-artifacts` job commits `signature.json` files back to `main`. Branch
protection must allow the `github-actions[bot]` actor (or a dedicated deploy
key / GitHub App installation token) to push. This is the same class of
repo-settings change as enabling GitHub Pages or marking a check required —
`docs/decision-log.md` entries 28/29 — and is deliberately not automated.

### 4. Backfill existing capabilities (registry#335)

Once steps 1–3 are done, run the backfill once (locally, or as a manual
`workflow_dispatch`):

```bash
ARTIFACT_SIGNING_ED25519_SECRET_KEY='<secret>' \
  python3 scripts/ci/sign_artifacts.py --all
git add capabilities catalog/signing-key.pub
git commit -m "chore(signing): backfill Ed25519 signatures for published artifacts"
```

Then, in the same PR, add the enforcement marker so completeness becomes a hard
CI gate:

```bash
echo "Signature completeness is CI-enforced (spec 007 Amendment FR-007)." \
  > capabilities/.signatures-enforced
```

## Verifying a signature

```bash
python3 - <<'EOF'
import json, urllib.request
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
d = "capabilities/classification/classification.outcome-resolve/1.0.0"
sig = json.load(open(f"{d}/signature.json"))
url = json.load(open(f"{d}/contract.json"))["artifact"]["url"]
data = urllib.request.urlopen(url).read()
Ed25519PublicKey.from_public_bytes(bytes.fromhex(sig["public_key_hex"])) \
    .verify(bytes.fromhex(sig["signature_hex"]), data)
print("OK")
EOF
```

The full `traverse-runtime` `verify_artifact()` end-to-end (spec 007 Amendment
SC-007) additionally depends on the traverse-side plumbing tracked in
`traverse-framework/traverse#1203`.
