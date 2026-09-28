// Verify-then-run core for text.detect-entities@1.1.1, shared unchanged by
// index.html (browser) and cli.js (Node >= 22). Same code, same signed bytes,
// both places: the point of the demo (registry#577 Track 2).
//
// Trust chain, checked in order before a single instruction runs:
//   1. contract.json (immutable registry record) names the artifact digest
//      and the pinned Hugging Face model revision.
//   2. The WASM bytes hash to exactly that digest.
//   3. The registry's Ed25519 signature over those bytes verifies against the
//      registry signing key pinned below (catalog/signing-key.pub) -- not
//      against whatever key signature.json claims.
// Then the module runs with a three-function host: stdin in, stdout out,
// exit code. No network, no filesystem, no clock.

export const CAPABILITY = {
  id: "text.detect-entities",
  version: "1.1.1",
  path: "capabilities/text/text.detect-entities/1.1.1",
};

// catalog/signing-key.pub
export const REGISTRY_SIGNING_KEY_HEX =
  "39d1745be4d54e30be1180454661e5d8826075692d86b86fb042d19d96cb8522";

const RECORDS = "https://raw.githubusercontent.com/traverse-framework/registry/main/";
const RELEASES = "https://github.com/traverse-framework/registry/releases/download/";
// CORS-open, digest-verified mirror of the release assets (registry#304).
const MIRROR = "https://registry.traverse-framework.com/";

const hexToBytes = (hex) => Uint8Array.from(hex.match(/../g), (b) => parseInt(b, 16));
const bytesToHex = (bytes) => Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");

async function fetchOk(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`GET ${url} -> HTTP ${response.status}`);
  return response;
}

/** Fetches, verifies, and compiles the capability. `onStep` receives one
 * human-readable line per completed check. Throws on any failed check. */
export async function loadVerifiedCapability(onStep = () => {}) {
  const contract = await (await fetchOk(`${RECORDS}${CAPABILITY.path}/contract.json`)).json();
  const signature = await (await fetchOk(`${RECORDS}${CAPABILITY.path}/signature.json`)).json();
  const [model] = contract.ai.models;
  onStep(
    `Contract ${contract.id}@${contract.version}: model ${model.huggingface_id}@${model.revision.slice(0, 8)} (${model.spdx_expression})`,
  );

  if (!contract.artifact.url.startsWith(RELEASES)) {
    throw new Error(`unexpected artifact host: ${contract.artifact.url}`);
  }
  const mirrorUrl = MIRROR + contract.artifact.url.slice(RELEASES.length);
  const bytes = new Uint8Array(await (await fetchOk(mirrorUrl)).arrayBuffer());
  const digest = `sha256:${bytesToHex(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)))}`;
  if (digest !== contract.artifact.digest) {
    throw new Error(`artifact digest ${digest} does not match contract ${contract.artifact.digest}`);
  }
  onStep(`Artifact ${(bytes.length / 1e6).toFixed(1)} MB, weights embedded; sha256 matches the contract (${digest.slice(7, 19)}…)`);

  if (signature.scheme !== "ed25519" || signature.public_key_hex !== REGISTRY_SIGNING_KEY_HEX) {
    throw new Error("signature is not from the pinned registry signing key");
  }
  const key = await crypto.subtle.importKey("raw", hexToBytes(REGISTRY_SIGNING_KEY_HEX), { name: "Ed25519" }, false, ["verify"]);
  const valid = await crypto.subtle.verify({ name: "Ed25519" }, key, hexToBytes(signature.signature_hex), bytes);
  if (!valid) throw new Error("Ed25519 signature does not verify over the artifact bytes");
  onStep(`Ed25519 signature verifies against the registry key (${REGISTRY_SIGNING_KEY_HEX.slice(0, 12)}…)`);

  const module = await WebAssembly.compile(bytes);
  return { contract, module, model };
}

class Exit {
  constructor(code) {
    this.code = code;
  }
}

/** Runs one call: `input` as JSON on stdin, JSON parsed from stdout. The
 * only host functions the artifact imports are these three. */
export async function runCapability(module, input) {
  const stdin = new TextEncoder().encode(JSON.stringify(input));
  let offset = 0;
  const stdout = [];
  const stderr = [];
  let memory;
  const iovecs = (iovs, count) => {
    const view = new DataView(memory.buffer);
    return Array.from({ length: count }, (_, i) => [view.getUint32(iovs + 8 * i, true), view.getUint32(iovs + 8 * i + 4, true)]);
  };
  const EBADF = 8;
  const host = {
    fd_read(fd, iovs, count, nreadPtr) {
      if (fd !== 0) return EBADF;
      let total = 0;
      for (const [ptr, len] of iovecs(iovs, count)) {
        const chunk = stdin.subarray(offset, offset + len);
        new Uint8Array(memory.buffer, ptr, chunk.length).set(chunk);
        offset += chunk.length;
        total += chunk.length;
        if (chunk.length < len) break;
      }
      new DataView(memory.buffer).setUint32(nreadPtr, total, true);
      return 0;
    },
    fd_write(fd, iovs, count, nwrittenPtr) {
      const sink = fd === 1 ? stdout : fd === 2 ? stderr : null;
      if (!sink) return EBADF;
      let total = 0;
      for (const [ptr, len] of iovecs(iovs, count)) {
        sink.push(new Uint8Array(memory.buffer, ptr, len).slice());
        total += len;
      }
      new DataView(memory.buffer).setUint32(nwrittenPtr, total, true);
      return 0;
    },
    proc_exit(code) {
      throw new Exit(code);
    },
  };
  const instance = await WebAssembly.instantiate(module, { wasi_snapshot_preview1: host });
  memory = instance.exports.memory;
  let code = 0;
  try {
    instance.exports._start();
  } catch (error) {
    if (!(error instanceof Exit)) throw error;
    code = error.code;
  }
  const text = (parts) => new TextDecoder().decode(new Uint8Array(parts.flatMap((p) => [...p])));
  if (code !== 0) throw new Error(`capability exited with code ${code}: ${text(stderr)}`);
  return JSON.parse(text(stdout));
}
