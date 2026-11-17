#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
c="$root/scripts/ci/pr_change_classification.sh"
out="$(printf '%s\n' 'docs/readme-extra.md' 'README.md' | bash "$c" --paths-stdin)"
echo "$out" | grep -q '^full_ci=false$'
out="$(printf '%s\n' 'capabilities/x/y/1.0.0/contract.json' | bash "$c" --paths-stdin)"
echo "$out" | grep -q '^full_ci=true$'
out="$(printf '%s\n' 'scripts/ci/foo.sh' | bash "$c" --paths-stdin)"
echo "$out" | grep -q '^full_ci=true$'
echo "pr_change_classification_test: ok"
