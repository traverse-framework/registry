#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
artifact="$script_dir/artifacts/pcm-transform.wasm"
mkdir -p "$script_dir/artifacts"
rustup run "$(rustup show active-toolchain | awk '{print $1}')" rustc "$script_dir/src/agent.rs" --edition=2024 --target wasm32-unknown-unknown --crate-type cdylib -O -C panic=abort -C strip=symbols --remap-path-prefix "$script_dir=/traverse-repo/audio-pcm-transform" -o "$artifact"
node --input-type=module -e '
import { readFile, writeFile } from "node:fs/promises";
const [manifestPath, artifactPath] = process.argv.slice(1);
const bytes = await readFile(artifactPath);
let hash = 0xcbf29ce484222325n;
for (const byte of bytes) hash = BigInt.asUintN(64, (hash ^ BigInt(byte)) * 0x100000001b3n);
const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
manifest.binary.expected_digest = `fnv1a64:${hash.toString(16).padStart(16, "0")}`;
await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
' "$script_dir/manifest.json" "$artifact"
