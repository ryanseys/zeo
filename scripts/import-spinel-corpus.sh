#!/usr/bin/env bash
# Import newly-added spinel corpus tests into zeo's conformance suite, TRIAGING
# each: a test zeo matches `ruby` on lands in tests/spinel/ (a passing corpus
# case); one zeo diverges on lands in tests/gaps/ (an XFAIL gap to grind down).
# Goldens are recorded from the ruby 4.0.6 oracle (never from spinel's own
# `.expected`, which can diverge). Idempotent: re-running skips tests already
# present and the ones listed in tests/spinel/REMOVED.txt.
#
#   scripts/import-spinel-corpus.sh <SPINEL_TEST_DIR>
#
# SPINEL_TEST_DIR is a local checkout of the predecessor project's test suite.
# The corpus it produces is already vendored in tests/spinel/, so this is a
# maintainer tool for pulling in NEW upstream cases, not a build step.
#
# Run from the repo root.
set -euo pipefail

SP="${1:-}"
[ -n "$SP" ] || { echo "usage: $0 <SPINEL_TEST_DIR>" >&2; exit 2; }
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TESTS="$ROOT/tests"
SPINEL="$TESTS/spinel"
GAPS="$TESTS/gaps"
RB="$(mise which ruby 2>/dev/null || echo ruby)"
FLAGS=(--disable-error_highlight --disable-did_you_mean)

[ -d "$SP" ] || { echo "no spinel test dir: $SP" >&2; exit 1; }

# The skiplist of deliberately-not-vendored spinel tests (bare stems).
mapfile -t REMOVED < <(grep -v '^#' "$SPINEL/REMOVED.txt" 2>/dev/null || true)
is_removed() { local s="${1%.rb}"; for r in "${REMOVED[@]}"; do [ "$r" = "$s" ] && return 0; done; return 1; }

# Bless <name>.rb's golden from ruby, run from the tests/ cwd with a RELATIVE
# source path (e.g. `spinel/<name>.rb`) so backtrace paths match the harness's
# normalization. Writes <name>.rb.expected, and <name>.rb.err.expected only if
# ruby wrote to stderr. A test whose oracle run RAISES is a legitimate golden
# (empty stdout + a backtrace on stderr), so ruby's exit status is ignored --
# without the `|| true` `set -e` would abort the whole import mid-bless.
bless() { # $1 = suite dir under tests/ ("spinel" or "gaps"), $2 = name.rb
  local rel="$1/$2" args stdin_file
  args="$(cat "$TESTS/$rel.args" 2>/dev/null || true)"
  stdin_file="$TESTS/$rel.stdin"
  ( cd "$TESTS"
    if [ -f "$stdin_file" ]; then
      "$RB" "${FLAGS[@]}" "$rel" $args < "$stdin_file" > "$rel.expected" 2>/tmp/import_err || true
    else
      "$RB" "${FLAGS[@]}" "$rel" $args > "$rel.expected" 2>/tmp/import_err || true
    fi )
  if [ -s /tmp/import_err ]; then cp /tmp/import_err "$TESTS/$rel.err.expected"
  else rm -f "$TESTS/$rel.err.expected"; fi
}

added=()
for rb in "$SP"/*.rb; do
  name="$(basename "$rb")"
  # Already vendored -- as a corpus case, a gap, or a flat top-level example.
  { [ -e "$SPINEL/$name" ] || [ -e "$GAPS/$name" ] || [ -e "$TESTS/$name" ]; } && continue
  is_removed "$name" && continue                      # skiplisted
  cp "$rb" "$SPINEL/$name"
  [ -f "$rb.args" ] && cp "$rb.args" "$SPINEL/$name.args"
  [ -f "$rb.stdin" ] && cp "$rb.stdin" "$SPINEL/$name.stdin"
  bless "spinel" "$name"
  added+=("$name")
done
echo "imported ${#added[@]} new tests; triaging against zeo ..."
[ ${#added[@]} -eq 0 ] && exit 0

# Run the corpus; any freshly-added test that FAILS is a gap.
fails="$(cargo nextest run -p zeo --test spinel --no-fail-fast 2>&1 \
  | sed -nE 's/.*spinel::([^ ]+)\.rb.*FAIL.*/\1/p; s/.*FAIL.*spinel::([^ ]+)\.rb.*/\1/p' | sort -u || true)"

corpus_n=0; gaps_n=0
for name in "${added[@]}"; do
  stem="${name%.rb}"
  if grep -qxF "$stem" <<<"$fails"; then
    for suf in rb rb.expected rb.err.expected rb.args rb.stdin; do
      [ -f "$SPINEL/$stem.$suf" ] && mv "$SPINEL/$stem.$suf" "$GAPS/"
    done
    gaps_n=$((gaps_n+1)); echo "  gap:    $name"
  else
    corpus_n=$((corpus_n+1))
  fi
done
echo "done: +$corpus_n corpus (pass), +$gaps_n gaps (fail), $(( ${#added[@]} - corpus_n - gaps_n )) other"
