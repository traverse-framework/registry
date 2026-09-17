# Third-party model notices

This file accompanies published model-backed capabilities in the Traverse
registry. Spec 025 `licensing` on a contract covers the **capability artifact
(WASM / crate source) only** — it does **not** inherit to models. Redistributors
must retain the notices below for any capability that embeds the listed weights.

Publisher posture for these packages: commercial use and redistribution are
**allowed** under each upstream permissive license when attribution/notice
conditions are met. This is maintainer-declared catalog guidance, not a law
firm opinion.

## openai/whisper-tiny (audio.transcribe-speech)

- SPDX: `MIT OR Apache-2.0` (dual-attested: GitHub `openai/whisper` LICENSE is
  MIT; Hugging Face model card metadata says Apache-2.0)
- Copyright: Copyright (c) 2022 OpenAI
- Sources:
  - https://github.com/openai/whisper/blob/main/LICENSE
  - https://huggingface.co/openai/whisper-tiny

MIT permission notice (from OpenAI Whisper LICENSE):

> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.

## snakers4/silero-vad (audio.detect-speech-segments; also audio.transcribe-speech@1.1.0)

- SPDX: `MIT`
- Source: https://github.com/snakers4/silero-vad/blob/master/LICENSE

## dslim/distilbert-NER (text.detect-entities, text.redact-entities)

- SPDX: `Apache-2.0`
- Source: https://huggingface.co/dslim/distilbert-NER

## sentence-transformers/paraphrase-MiniLM-L3-v2 (report.translate-fr-semantic)

- SPDX: `Apache-2.0`
- Sources:
  - https://huggingface.co/sentence-transformers/paraphrase-MiniLM-L3-v2
  - https://huggingface.co/Xenova/paraphrase-MiniLM-L3-v2

## minishlab/potion-base-32M (report.summarize-semantic)

- SPDX: `MIT`
- Source: https://huggingface.co/minishlab/potion-base-32M

Machine-readable catalog mirror: [`model-attribution.json`](./model-attribution.json).
