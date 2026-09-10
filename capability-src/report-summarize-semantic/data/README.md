# Model data for report.summarize-semantic

Generate the int8 table locally (not committed — ~31 MB):

```bash
python3 scripts/model/prepare_potion_base_32m_int8.py
python3 scripts/model/prepare_potion_base_32m_int8.py --check-only
```

Pinned sha256: `159b2efd704766be0bf2a035c3f49fe62d588d29643c7025f7ce38b0b7584cd1`

Also published at:
https://github.com/traverse-framework/registry/releases/download/artifacts/report.summarize-semantic-1.0.0/potion-base-32M-int8.bin

Release WASM builds require `--features full-model` with this file present.
