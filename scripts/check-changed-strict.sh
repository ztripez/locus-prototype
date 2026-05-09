#!/usr/bin/env bash
set -euo pipefail

# Canonical dogfood / CI gate for Epic #1:
# fail only on new violations introduced in changed files.

WORKSPACE="${1:-.}"
BASELINE="${LOCUS_BASELINE:-}"

if [[ -n "$BASELINE" ]]; then
  echo "Running: locus check --workspace $WORKSPACE --changed --baseline $BASELINE --agent-strict"
  exec locus check --workspace "$WORKSPACE" --changed --baseline "$BASELINE" --agent-strict
else
  echo "Running: locus check --workspace $WORKSPACE --changed --agent-strict"
  exec locus check --workspace "$WORKSPACE" --changed --agent-strict
fi
