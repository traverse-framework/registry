# Personas Directory

Governed persona records referenced by capability `use_cases[].persona_ref`.
Layout and field rules: [`specs/017-persona-registry/spec.md`](../specs/017-persona-registry/spec.md).

```text
personas/<persona-id>/<version>/persona.json
```

Each version is immutable once merged — correct mistakes by publishing a new version, never by editing.

## Scaffold a new persona

Use the helper so `distinguished_from` resolves against the live tree and
`scripts/ci/capability_validation.py`'s `validate_persona` checks pass locally
before you open a PR:

```bash
# Interactive
bash scripts/scaffold/new-persona.sh

# Non-interactive
bash scripts/scaffold/new-persona.sh --non-interactive \
  --id example-persona \
  --version 1.0.0 \
  --name "Example Persona" \
  --summary "One-sentence summary." \
  --description "Fuller paragraph of goals and constraints." \
  --distinguished-from 'platform-security-engineer:Differs by focusing on X, not Y.'
```

`distinguished_from` entries use `persona_id:how` and must name an already-registered persona whenever any personas exist (they do today). See also the publisher checklist in [`capabilities/README.md`](../capabilities/README.md).
