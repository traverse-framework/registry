#!/usr/bin/env node
// Local run of the same verified bytes index.html runs in the browser.
// Usage: node cli.js "Elon Musk founded SpaceX in California."
import { loadVerifiedCapability, runCapability } from "./ner-demo.js";

const text = process.argv.slice(2).join(" ") || "Sarah Chen from Acme Corp will follow up with the Berlin team next week.";

const { module } = await loadVerifiedCapability((step) => console.error(`✓ ${step}`));
const started = performance.now();
const output = await runCapability(module, { text });
console.error(`✓ Ran locally in ${Math.round(performance.now() - started)} ms (no network after load)\n`);
console.log(JSON.stringify(output, null, 2));
