#!/usr/bin/env bash
# Promote a FIXED gap out of tests/gaps/ into the passing suite.
#
# When a gap starts matching ruby, `cargo test --test gaps` fails it with
# "GAP FIXED -- promote". This moves the gap's `.rb` and every sidecar
# (.rb.expected/.rb.err.expected/.rb.args/.rb.stdin) into tests/ -- the
# zeo-authored golden suite (the `examples` test target) -- and confirms it
# passes there. tests/spinel/ is NOT a promotion target: it mirrors the vendored
# spinel corpus, and a spinel-origin gap re-promotes automatically the next time
# scripts/import-spinel-corpus.sh triages it.
#
#   scripts/promote-gap.sh <gap-stem>        # e.g. conditional_reopen_adds_method
#
# Run from anywhere; paths resolve against the repo root.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GAPS="$ROOT/tests/gaps"
DEST="$ROOT/tests"

stem="${1:?usage: promote-gap.sh <gap-stem>}"
stem="${stem%.rb}"
[ -f "$GAPS/$stem.rb" ] || { echo "no such gap: tests/gaps/$stem.rb" >&2; exit 1; }

moved=()
for suf in rb rb.expected rb.err.expected rb.args rb.stdin; do
  if [ -f "$GAPS/$stem.$suf" ]; then
    mv "$GAPS/$stem.$suf" "$DEST/"
    moved+=("$stem.$suf")
  fi
done
echo "promoted tests/gaps/$stem.rb -> tests/ (${#moved[@]} files: ${moved[*]})"

echo "verifying it passes in the examples suite ..."
( cd "$ROOT" && cargo test -p zeo --test examples -- "$stem" )
