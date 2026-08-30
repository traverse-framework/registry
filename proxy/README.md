# Registry Execution Proxy

Implements `specs/020-public-execution-proxy` (Approved) — a thin Cloudflare
Worker that lets `discover.html`'s browser client execute one verified,
published capability with no credential of its own, by holding the real
`traverse-cli serve` admin JWT privately and forwarding validated requests.

Decisions behind every choice below (hosting, JWT lifecycle, rate limit,
CORS, language) are recorded in `docs/decision-log.md` entry 73 — this file
is the *how*, that entry is the *why*.

**This is a runbook for a human operator.** Nothing in this repo can create
the Cloudflare/Oracle Cloud accounts, mint the real signing key, or deploy
to infrastructure it has no credentials for — every step below is manual.

## Prerequisites

- A Cloudflare account (free tier is sufficient) with `wrangler` installed
  (`npm install -g wrangler`) and logged in (`wrangler login`).
- An Oracle Cloud account for the Always Free VM `traverse-cli serve` runs
  on (any provider works; this is the one with a genuine no-cost tier at
  this traffic level — see decision-log entry 73).
- `traverse-cli` available on that VM (build from
  `traverse-framework/traverse`, or install per that repo's own docs).
- Python 3 with `pyjwt` and `cryptography` installed, for the one-time JWT
  minting step (`pip install pyjwt cryptography`) — can be run on any
  machine, not necessarily the VM itself.

## Step 1 — Stand up `traverse-cli serve`

Verified working end to end against the live published registry (2026-08-30)
after the host-artifact-preparation pipeline landed:
`traverse#1214` (`prepare-public-bundle`), `traverse#1218` (referenced event
contracts), `traverse#1221` / registry Spec 021 (`serve` trusts a signed
public bundle's publish-time governance). Needs `traverse-cli` built from
`traverse-framework/traverse` at or after `8ff0157`, i.e. `traverse-registry`
`>= 0.17.0`.

On the Oracle Cloud VM:

```bash
# 1. Sync this registry's published public index into a local workspace.
traverse-cli registry sync --workspace proxy --json

# 2. Prepare a serve-ready bundle: fetch + digest/signature-verify each
#    non-deprecated capability's contract.json and signature.json, plus the
#    event contracts they emit, and emit the bundle.json manifest.
traverse-cli registry prepare-public-bundle \
  --synced-state .traverse/workspaces/proxy/registry/public/index.json \
  --out .traverse/workspaces/proxy/registry/prepared

# 3. Materialize (download + digest-verify) the artifacts the bundle references.
traverse-cli registry materialize \
  --registry-state .traverse/workspaces/proxy/registry/prepared/bundle.json \
  --out .traverse/workspaces/proxy/artifacts

# Keep fresh: re-run all three on a schedule (daily cron / systemd timer) so
# newly published or deprecated capabilities show up without a manual
# restart. Ordinary operational upkeep spec 020 leaves to whoever operates
# the paired serve instance -- not automated here.
```

Then start `serve` bound to a **non-loopback** address — `traverse-cli`
automatically runs in `bearer-required` auth mode the moment the bind
address isn't loopback (127.0.0.1/::1); there is no `--auth bearer-required`
flag, and passing `--auth dev-any` would defeat the whole point (LAN/public
callers without a token). Verified directly against
`crates/traverse-cli/src/http_api.rs`'s own auth-mode resolution before
writing this, not assumed:

```bash
TRAVERSE_JWT_VERIFICATION_KEY=<hex Ed25519 public key from Step 2> \
  traverse-cli serve \
  --bind 0.0.0.0:8787 \
  --registry-state .traverse/workspaces/proxy/registry/prepared/bundle.json \
  --artifact-state .traverse/workspaces/proxy/artifacts/artifact-state.json
```

Put this behind a process supervisor (systemd unit, or equivalent) so it
restarts on crash/reboot — this VM is self-managed, not a platform with
built-in supervision (the tradeoff decision-log entry 73 accepted for
zero hosting cost). Only the Worker (Step 3) should ever be able to reach
this port — firewall it to the Worker's egress if your provider makes that
practical; at minimum, keep the bind port off any well-known port scanners
target by default.

## Step 2 — Mint the admin JWT

There is no `traverse-cli` command that mints a `bearer-required` admin
token — confirmed directly against `crates/traverse-cli/src/http_api.rs`
before writing this runbook, not assumed. This is a real, if narrow, gap:
minting one today means signing it yourself. Run this **once**, locally,
anywhere with Python:

```python
#!/usr/bin/env python3
"""Mint the proxy's one-time admin JWT. Run once; store nowhere except the
two places printed below (the serve host's env, and the Worker's secret)."""
import time
import jwt
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives import serialization

private_key = Ed25519PrivateKey.generate()
public_key_hex = private_key.public_key().public_bytes(
    serialization.Encoding.Raw, serialization.PublicFormat.Raw
).hex()
private_key_pem = private_key.private_bytes(
    serialization.Encoding.PEM,
    serialization.PrivateFormat.PKCS8,
    serialization.NoEncryption(),
)

token = jwt.encode(
    {
        "sub": "registry-execution-proxy",
        "traverse_admin": True,
        "iat": int(time.time()),
        # No "exp": decision-log entry 73 chose mint-once-long-expiry.
        # Add one here (e.g. "exp": int(time.time()) + 365 * 24 * 3600)
        # if you'd rather cap it and rotate manually instead.
    },
    private_key_pem,
    algorithm="EdDSA",
)

print("TRAVERSE_JWT_VERIFICATION_KEY (set on the serve host, Step 1):")
print(public_key_hex)
print()
print("ADMIN_JWT (set via `wrangler secret put ADMIN_JWT`, Step 3):")
print(token)
```

The private key exists only in this script's process memory — it is never
printed or saved. Losing it is fine (it only ever signed one token); if the
printed `ADMIN_JWT` itself ever leaks, generate a new keypair, mint a new
token, update both the serve host's `TRAVERSE_JWT_VERIFICATION_KEY` and the
Worker's `ADMIN_JWT` secret, and restart `serve`.

## Step 3 — Deploy the Worker

From this directory:

```bash
# One-time: fill in wrangler.toml's SERVE_URL with Step 1's real address
# and ALLOWED_ORIGIN with the real discover.html-hosting origin (both are
# placeholders in the committed file — see wrangler.toml's own comments).

wrangler secret put ADMIN_JWT
# paste the token Step 2 printed, then Enter

wrangler deploy
```

`wrangler deploy` provisions the `[[ratelimits]]` binding declared in
`wrangler.toml` automatically on first deploy; if `namespace_id = "1"`
collides with an existing rate limiter in your account, wrangler's error
will say so — pick a different number and redeploy.

## Step 4 — Verify (spec 020 SC-001)

```bash
curl -s -X POST https://<your-worker-subdomain>.workers.dev/execute \
  -H 'Content-Type: application/json' \
  -d '{
    "entrypoint_kind": "capability",
    "id": "core.transition-action-status",
    "version": "1.4.0",
    "request": {
      "action_item_id": "item-001",
      "actor_id": "user-ada",
      "owner_id": "user-ada",
      "current_status": "open",
      "requested_status": "in_progress",
      "transition_config": {
        "version": "1.0",
        "allowed_transitions": {"open": ["in_progress"]},
        "owner_only": true
      }
    }
  }'
```

A genuine execution result (not an error) with no credential of any kind
supplied by the caller is exactly SC-001. Also worth checking:

- A request naming a deprecated or nonexistent capability gets
  `capability_not_found`, not a call to `serve` (SC-002).
- Six rapid requests from the same IP: the sixth gets `rate_limited` (FR-003,
  5/minute).
- Nothing in any response — success or error — ever contains `ADMIN_JWT`'s
  value (FR-005). Skim a few real responses to confirm, don't just trust
  the code.

## What this Worker deliberately does NOT do

- It does not mint or rotate `ADMIN_JWT` itself (Step 2 is manual, by
  design — see decision-log entry 73).
- It does not keep `serve`'s registry-state/artifact-state fresh — that's
  Step 1's cron/systemd-timer note, an operational task per spec 020's own
  Assumptions, not something the Worker or this repo automates.
- It validates against `catalog.json`, not a specific `index.json` GitHub
  Release asset — see the doc comment on `CATALOG_URL` in `src/lib.rs` for
  why (no reliable "latest index release" URL exists without an
  unauthenticated-GitHub-API call from Cloudflare's shared edge IPs).
