#!/usr/bin/env bash
# Every environment variable the docs name must have a reader in the tree.
#
# A documented variable with no reader is a silent no-op: the reader was
# renamed or deleted and the instruction outlived it. Three of those were
# found at once in 2026-08 -- `ZEO_BLESS=1` had been replaced by
# `ZEO_BLESS_FROM_XTASK`, and two ledgers plus a doc still told the reader to
# use the old spelling, which does nothing at all.
#
# A variable named only to say it does NOT work is exempt: list it in
# HISTORICAL below.
set -euo pipefail
cd "$(dirname "$0")/.."

# Spellings the docs mention only to say they are retired.
HISTORICAL="ZEO_BLESS"

DOCS=(README.md CONTRIBUTING.md)
while IFS= read -r f; do DOCS+=("$f"); done < <(find docs conformance tests -name '*.md' -o -name '*.tsv' 2>/dev/null)

named=$(grep -ohE '\bZEO_[A-Z0-9_]+\b' "${DOCS[@]}" 2>/dev/null | sort -u)

missing=""
for v in $named; do
  case " $HISTORICAL " in *" $v "*) continue ;; esac
  # A reader spells the name as a string literal: env::var("X"), var_os("X").
  if ! grep -rqF "\"$v\"" crates/ scripts/ 2>/dev/null; then
    missing="$missing  $v\n"
  fi
done

if [ -n "$missing" ]; then
  echo "::error::documented environment variables with no reader in crates/:"
  printf "$missing"
  echo "Either restore the reader, correct the spelling, or add it to HISTORICAL in $0."
  exit 1
fi
echo "every documented ZEO_* variable has a reader"
