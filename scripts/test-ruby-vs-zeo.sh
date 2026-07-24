#!/usr/bin/env bash
# Run a Ruby snippet through BOTH `ruby` (the oracle) and `zeo`, show each
# side's stdout/stderr/exit, and report whether they match. On a DIVERGENCE it
# captures the snippet as a new XFAIL gap under tests/gaps/ (golden blessed from
# ruby, exactly like scripts/import-spinel-corpus.sh) and confirms the gaps
# harness agrees it's a real gap. On a MATCH it writes nothing.
#
#   scripts/test-ruby-vs-zeo.sh 'puts 1 + 1'            # inline code
#   scripts/test-ruby-vs-zeo.sh -f snippet.rb          # from a file
#   pbpaste | scripts/test-ruby-vs-zeo.sh -            # from stdin
#
#   -n NAME     gap name to use on divergence (default: gap_snippet)
#   --no-gap    just compare; never write a gap file
#
# The gaps harness (`cargo nextest run -p zeo --test gaps`) is the arbiter of
# whether a divergence is a real gap: after writing + blessing we run it and, if
# it reports "GAP FIXED" (zeo actually matches under the harness's source-path
# normalization), we remove the file and tell you to put it in tests/ instead.
#
# Run from anywhere; paths resolve against the repo root.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TESTS="$ROOT/tests"
GAPS="$TESTS/gaps"
RB="$(mise which ruby 2>/dev/null || echo ruby)"
FLAGS=(--disable-error_highlight --disable-did_you_mean)

name="gap_snippet"
make_gap=1
src=""
mode="inline"

while [ $# -gt 0 ]; do
  case "$1" in
    -n) name="$2"; shift 2 ;;
    --no-gap) make_gap=0; shift ;;
    -f) mode="file"; src="$2"; shift 2 ;;
    -) mode="stdin"; shift ;;
    -h|--help) sed -n '2,25p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) mode="inline"; src="$1"; shift ;;
  esac
done
name="${name%.rb}"

# Resolve the snippet into a temp .rb.
tmp="$(mktemp -t zeo_snippet.XXXXXX).rb"
trap 'rm -f "$tmp" "$tmp".out.rb "$tmp".out.zeo "$tmp".err.rb "$tmp".err.zeo' EXIT
case "$mode" in
  inline) printf '%s\n' "$src" > "$tmp" ;;
  file)   cat "$src" > "$tmp" ;;
  stdin)  cat > "$tmp" ;;
esac

echo "=== snippet ==="
cat "$tmp"
echo

# --- ruby oracle ---
"$RB" "${FLAGS[@]}" "$tmp" > "$tmp.out.rb" 2> "$tmp.err.rb"
rb_exit=$?

# --- zeo (compile + run inline) ---
( cd "$ROOT" && cargo run --release -q -p zeo -- --no-report -e "$(cat "$tmp")" ) \
  > "$tmp.out.zeo" 2> "$tmp.err.zeo"
zeo_exit=$?

show() { # $1 = label, $2 = out file, $3 = err file, $4 = exit
  echo "--- $1 (exit $4) ---"
  echo "stdout:"; cat "$2"
  if [ -s "$3" ]; then echo "stderr:"; cat "$3"; fi
}
show "ruby" "$tmp.out.rb" "$tmp.err.rb" "$rb_exit"
echo
show "zeo"  "$tmp.out.zeo" "$tmp.err.zeo" "$zeo_exit"
echo

# Quick verdict on stdout + exit (the harness reconciles the fine print).
if diff -q "$tmp.out.rb" "$tmp.out.zeo" >/dev/null && [ "$rb_exit" = "$zeo_exit" ]; then
  echo "✅ MATCH (stdout + exit). Not a gap — belongs in tests/ if you want to keep it."
  exit 0
fi

echo "❌ DIVERGE (stdout diff below):"
diff "$tmp.out.rb" "$tmp.out.zeo" || true
echo

[ "$make_gap" = 1 ] || { echo "(--no-gap: not writing a gap file)"; exit 0; }

# Capture as a gap: copy the .rb in, bless the golden from ruby at cwd=tests/
# with a RELATIVE path so backtrace source paths match the harness normalizer.
dest="$GAPS/$name.rb"
[ -e "$dest" ] && { echo "refusing to overwrite existing tests/gaps/$name.rb (use -n)"; exit 1; }
cp "$tmp" "$dest"
( cd "$TESTS"
  if "$RB" "${FLAGS[@]}" "gaps/$name.rb" > "gaps/$name.rb.expected" 2>/tmp/zeo_snip_err; then :; fi
  if [ -s /tmp/zeo_snip_err ]; then cp /tmp/zeo_snip_err "gaps/$name.rb.err.expected"
  else rm -f "gaps/$name.rb.err.expected"; fi )
echo "wrote tests/gaps/$name.rb (+ blessed golden from ruby)"

echo "confirming the XFAIL holds via the gaps harness ..."
if ( cd "$ROOT" && cargo nextest run -p zeo --test gaps -- "$name" 2>&1 ) | tee /tmp/zeo_gap_run | grep -q 'GAP FIXED'; then
  echo
  echo "⚠  the gaps harness says zeo actually MATCHES ruby (under source-path"
  echo "   normalization) — this is NOT a gap. Removing it; promote to tests/ instead."
  for suf in rb rb.expected rb.err.expected; do rm -f "$GAPS/$name.$suf"; done
  exit 1
fi
echo "✅ gap tests/gaps/$name.rb is a valid XFAIL (zeo diverges). Grind it down, then:"
echo "   scripts/promote-gap.sh $name"
