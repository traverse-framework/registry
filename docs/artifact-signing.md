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

### 3. (Optional) Let Actions push to `main` for the ongoing case

On merge to `main`, the `sign-artifacts` job tries to commit new `signature.json`
files back directly. That direct push needs the `github-actions[bot]` actor to be
allowed past the branch rules on `main`. It is **not required** to activate
signing:

- If the bot is not allowed to push, the job logs a warning and still succeeds
  (`continue-on-error`). The signed files are always uploaded as the
  `artifact-signatures` workflow artifact for a maintainer to download and open a
  normal PR with.
- New-capability publishes are infrequent, so handling them by PR is low-friction.
  Grant the bypass (or provision a bot PAT / GitHub App token) only if you want
  the per-publish commit to be fully automatic.

Weakening the `traverse-governance-*` rulesets for this is a deliberate
governance call, not a routine toggle — treat it as one.

### 4. Backfill existing capabilities (registry#335)

Once the secret (step 2) exists, run the backfill once via CI — it uses the
repo secret and uploads the signed files as the `artifact-signatures` artifact:

```bash
gh workflow run CI --repo traverse-framework/registry -f sign_mode=all
```

Then download that artifact and open a PR with its contents:

```bash
run_id=$(gh run list --repo traverse-framework/registry --workflow CI \
  --event workflow_dispatch -L1 --json databaseId --jq '.[0].databaseId')
gh run download "$run_id" --repo traverse-framework/registry -n artifact-signatures -D .
git checkout -b chore/backfill-signatures
git add capabilities catalog/signing-key.pub
git commit -m "chore(signing): backfill Ed25519 signatures for published artifacts"
```

(Or run `scripts/ci/sign_artifacts.py --all` locally with the secret in the
environment, if you have a copy of it.)

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
