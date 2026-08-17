#!/usr/bin/env bash
# Sync zeo's conformance suite with the predecessor project's test corpus,
# TRIAGING each case: a test zeo matches `ruby` on lands in tests/spinel/ (a
# passing corpus case); one zeo diverges on lands in tests/gaps/ (an XFAIL gap
# to grind down). Goldens are recorded from the ruby 4.0.6 oracle (never from
# spinel's own `.expected`, which can diverge). Idempotent.
#
#   scripts/import-spinel-corpus.sh [--refresh] [--no-triage] <SPINEL_TEST_DIR>
#
# Default (import) pulls in tests not vendored yet.
# `--refresh` handles the other drift axis: a test already vendored whose
# UPSTREAM BODY has since changed. It overwrites zeo's copy and re-blesses.
# `--no-triage` copies and blesses but does not run the suite, so a sync that
# does both modes pays for one corpus run instead of two. Triage by hand after:
# every touched test that FAILS `--test spinel` belongs in tests/gaps/.
#
# Two skiplists, both bare stems with `#` comments, both honoured by both modes:
#   tests/spinel/REMOVED.txt  never vendor this test at all
#   tests/spinel/PORTED.txt   zeo's copy is deliberately not upstream's --
#                             never refresh its body
#
# SPINEL_TEST_DIR is a local checkout of the predecessor project's test suite.
# The corpus it produces is already vendored in tests/spinel/, so this is a
# maintainer tool for pulling upstream changes, not a build step.
#
# Run from the repo root.
set -euo pipefail

MODE=import
TRIAGE=1
SP=""
while [ $# -gt 0 ]; do
  case "$1" in
    --refresh) MODE=refresh; shift ;;
    --no-triage) TRIAGE=0; shift ;;
    -h|--help) sed -n '2,25p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "unknown flag: $1" >&2; exit 2 ;;
    *) SP="$1"; shift ;;
  esac
done
[ -n "$SP" ] || { echo "usage: $0 [--refresh] <SPINEL_TEST_DIR>" >&2; exit 2; }
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

# The protect-list: zeo's copy is deliberately not upstream's, so --refresh
# must leave it alone. See tests/spinel/PORTED.txt for the reason on each.
mapfile -t PORTED < <(grep -v '^#' "$SPINEL/PORTED.txt" 2>/dev/null || true)
is_ported() { local s="${1%.rb}"; for r in "${PORTED[@]}"; do [ "$r" = "$s" ] && return 0; done; return 1; }

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

# Copy <name>.rb and its argv/stdin sidecars out of the upstream tree.
vendor() { # $1 = upstream .rb path, $2 = name.rb
  cp "$1" "$SPINEL/$2"
  [ -f "$1.args" ] && cp "$1.args" "$SPINEL/$2.args"
  [ -f "$1.stdin" ] && cp "$1.stdin" "$SPINEL/$2.stdin"
  bless "spinel" "$2"
}

touched=()
if [ "$MODE" = import ]; then
  for rb in "$SP"/*.rb; do
    name="$(basename "$rb")"
    # Already vendored -- as a corpus case, a gap, or a flat top-level example.
    { [ -e "$SPINEL/$name" ] || [ -e "$GAPS/$name" ] || [ -e "$TESTS/$name" ]; } && continue
    is_removed "$name" && continue                      # skiplisted
    vendor "$rb" "$name"
    touched+=("$name")
  done
  echo "imported ${#touched[@]} new tests; triaging against zeo ..."
else
  skipped=0
  for vend in "$SPINEL"/*.rb; do
    name="$(basename "$vend")"
    rb="$SP/$name"
    [ -f "$rb" ] || continue                            # zeo-only, or deleted upstream
    cmp -s "$vend" "$rb" && continue                    # body already identical
    if is_ported "$name" || is_removed "$name"; then
      skipped=$((skipped+1)); continue
    fi
    vendor "$rb" "$name"
    touched+=("$name")
  done
  echo "refreshed ${#touched[@]} bodies ($skipped protected); triaging against zeo ..."
fi
[ ${#touched[@]} -eq 0 ] && exit 0
if [ "$TRIAGE" = 0 ]; then
  printf '%s\n' "${touched[@]}" > /tmp/spinel_touched.txt
  echo "--no-triage: names written to /tmp/spinel_touched.txt; run the suite yourself"
  exit 0
fi

# Run the corpus; any freshly-touched test that FAILS is a gap. The target
# lives in the zeo-tests crate -- naming the wrong package makes this command
# error, `|| true` swallow it, and every touched test be called a pass.
fails="$(cargo nextest run -p zeo-tests --test spinel --no-fail-fast 2>&1 \
  | sed -nE 's/.*spinel::([^ ]+)\.rb.*FAIL.*/\1/p; s/.*FAIL.*spinel::([^ ]+)\.rb.*/\1/p' | sort -u || true)"

corpus_n=0; gaps_n=0
for name in "${touched[@]}"; do
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
echo "done: +$corpus_n corpus (pass), +$gaps_n gaps (fail), $(( ${#touched[@]} - corpus_n - gaps_n )) other"
