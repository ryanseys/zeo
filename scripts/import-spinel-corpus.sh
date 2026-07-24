#!/usr/bin/env bash
# Import newly-added spinel corpus tests into zeo's conformance suite, TRIAGING
# each: a test zeo matches `ruby` on lands in conformance/corpus/test/ (a passing
# corpus case); one zeo diverges on lands in conformance/gaps/ (an XFAIL gap to
# grind down). Goldens are recorded from the ruby 4.0.5 oracle (never from
# spinel's own `.expected`, which can diverge). Idempotent: re-running skips
# tests already present and the ones listed in conformance/corpus/REMOVED.txt.
#
#   scripts/import-spinel-corpus.sh [SPINEL_TEST_DIR]   # default ~/dev/spinel/test
#
# Run from the repo root.
set -euo pipefail

SP="${1:-$HOME/dev/spinel/test}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CORPUS="$ROOT/conformance/corpus"
GAPS="$ROOT/conformance/gaps"
RB="$(mise which ruby 2>/dev/null || echo ruby)"
FLAGS=(--disable-error_highlight --disable-did_you_mean)

[ -d "$SP" ] || { echo "no spinel test dir: $SP" >&2; exit 1; }

# The skiplist of deliberately-not-vendored spinel tests (bare stems).
mapfile -t REMOVED < <(grep -v '^#' "$CORPUS/REMOVED.txt" 2>/dev/null || true)
is_removed() { local s="${1%.rb}"; for r in "${REMOVED[@]}"; do [ "$r" = "$s" ] && return 0; done; return 1; }

# Bless <name>.rb's golden from ruby, run WITH the corpus cwd + a RELATIVE source
# path so backtrace paths come out as `test/<name>.rb` (matching the harness's
# normalization). Writes <name>.rb.expected, and <name>.rb.err.expected only if
# ruby wrote to stderr.
bless() { # $1 = dir (relative to $CORPUS, e.g. "test" or "../gaps"), $2 = name.rb
  local rel="$1/$2" args stdin_file
  args="$(cat "$CORPUS/$rel.args" 2>/dev/null || true)"
  stdin_file="$CORPUS/$rel.stdin"
  ( cd "$CORPUS"
    if [ -f "$stdin_file" ]; then
      "$RB" "${FLAGS[@]}" "$rel" $args < "$stdin_file" > "$rel.expected" 2>/tmp/import_err
    else
      "$RB" "${FLAGS[@]}" "$rel" $args > "$rel.expected" 2>/tmp/import_err
    fi )
  if [ -s /tmp/import_err ]; then cp /tmp/import_err "$CORPUS/$rel.err.expected"
  else rm -f "$CORPUS/$rel.err.expected"; fi
}

added=()
for rb in "$SP"/*.rb; do
  name="$(basename "$rb")"
  [ -e "$CORPUS/test/$name" ] && continue            # already vendored
  is_removed "$name" && continue                     # skiplisted
  cp "$rb" "$CORPUS/test/$name"
  [ -f "$rb.args" ] && cp "$rb.args" "$CORPUS/test/$name.args"
  [ -f "$rb.stdin" ] && cp "$rb.stdin" "$CORPUS/test/$name.stdin"
  bless "test" "$name"
  added+=("$name")
done
echo "imported ${#added[@]} new tests; triaging against zeo ..."
[ ${#added[@]} -eq 0 ] && exit 0

# Run the corpus; any freshly-added test that FAILS is a gap.
fails="$(cargo nextest run -p zeo --test corpus --no-fail-fast 2>&1 \
  | sed -nE 's/.*corpus::([^ ]+)\.rb.*FAIL.*/\1/p; s/.*FAIL.*corpus::([^ ]+)\.rb.*/\1/p' | sort -u || true)"

corpus_n=0; gaps_n=0
for name in "${added[@]}"; do
  stem="${name%.rb}"
  if grep -qxF "$stem" <<<"$fails"; then
    for suf in rb rb.expected rb.err.expected rb.args rb.stdin; do
      [ -f "$CORPUS/test/$stem.$suf" ] && mv "$CORPUS/test/$stem.$suf" "$GAPS/"
    done
    gaps_n=$((gaps_n+1)); echo "  gap:    $name"
  else
    corpus_n=$((corpus_n+1))
  fi
done
echo "done: +$corpus_n corpus (pass), +$gaps_n gaps (fail), $(( ${#added[@]} - corpus_n - gaps_n )) other"
