#!/usr/bin/env bash
# Front-end A/B: compile a panel of REAL, heavy gems with two zeo binaries and
# compare wall time.
#
# Why this exists: the bench corpus is the wrong instrument for front-end work.
# Its largest program reaches 411 classes and the whole mro pass reports 0 ms,
# so a front-end change measures as noise there no matter how real it is. The
# gem corpus is where the front end actually gets stressed -- the panel below
# spans 3 s to 24 s of pure front-end-plus-codegen (no rustc runs at all) and
# 66 MB to 315 MB of emitted Rust.
#
#   crates/xtask/frontend-ab.sh <baseline-zeo> <candidate-zeo> [runs] [panel]
#
# Reads `codegen_ms` out of conformance/gem-probe-timings.tsv, which is the
# wall time of the whole `zeo --emit-rust` subprocess. Restores the ledger
# afterwards -- probing rewrites it and those rows are not the measurement.
#
# TWO PANELS, because they answer different questions:
#
#   quick  ~11 s a run. For iterating. Cannot resolve better than about 3%:
#          measured run-to-run spread on this panel is +-3% and best-of-2 does
#          not average it out, so a null result here means "no big regression",
#          never "no effect".
#   heavy  ~119 s a run, and the only panel a single-digit claim may rest on.
#          The gems are large enough that a superlinear term has room to show
#          itself at all -- which is the whole point, since these changes are
#          invisible on anything smaller.
#
# Do NOT use the bench corpus for front-end work. Its largest program reaches
# 411 classes and the whole mro pass reports 0 ms.
#
# BUILD THE BASELINE AT THIS SAME PATH, not in a worktree. A zeo binary bakes
# in the location of its sibling `gems/` directory, and codegen emits those
# absolute paths inline (`record_const_location`, `seed_loaded_features`). A
# worktree baseline therefore emits LONGER strings than the candidate and loses
# time to it -- a confound worth several percent, which is the same size as the
# effects being measured. Check for it: both binaries must emit byte-identical
# Rust for the same input.
#
#   git checkout --detach <baseline-sha> && cargo build --release -p zeo
#   cp target/release/zeo target/ab/zeo-before
#   git checkout <branch>          && cargo build --release -p zeo
#   cp target/release/zeo target/ab/zeo-after
#
# That confound is not hypothetical -- it is why this warning exists. Measuring
# 65567ff6 against adf61271 gave -7.23% with a worktree baseline and -0.63%
# with both binaries built here, because the worktree baseline's own openc3
# time was inflated from 65,633 ms to 70,549 ms by the longer paths alone. A
# result this harness produces is only worth reporting once the byte-identical
# check above has passed.

set -euo pipefail

BASE=${1:?usage: frontend-ab.sh <baseline-zeo> <candidate-zeo> [runs] [quick|heavy]}
CAND=${2:?usage: frontend-ab.sh <baseline-zeo> <candidate-zeo> [runs] [quick|heavy]}
RUNS=${3:-2}
PANEL_NAME=${4:-quick}

case "$PANEL_NAME" in
    quick) PANEL=(activesupport moderation_api ruflet_core lithic) ;;
    heavy) PANEL=(openc3 hyper-spec) ;;
    *) echo "unknown panel: $PANEL_NAME (quick|heavy)" >&2; exit 2 ;;
esac

TIMINGS=conformance/gem-probe-timings.tsv
OUT=$(mktemp -d)
trap 'git checkout -- conformance/ 2>/dev/null || true' EXIT

probe_one() {   # $1 = binary, $2 = gem -> best-of-RUNS ms on stdout
    local best=""
    for _ in $(seq "$RUNS"); do
        cargo run -q -p xtask -- gem-probe "$2" --zeo "$1" --refresh >/dev/null 2>&1
        local ms
        ms=$(awk -F'\t' -v g="$2" '$1==g {v=$4} END {print v}' "$TIMINGS")
        [ -z "$best" ] && best=$ms
        [ "$ms" -lt "$best" ] && best=$ms
    done
    echo "$best"
}

printf '%-28s %10s %10s %8s\n' gem baseline candidate delta
for gem in "${PANEL[@]}"; do
    b=$(probe_one "$BASE" "$gem")
    c=$(probe_one "$CAND" "$gem")
    printf '%-28s %9sms %9sms %7.1f%%\n' \
        "$gem" "$b" "$c" "$(python3 -c "print(($c/$b-1)*100)")"
    echo "$b $c" >> "$OUT/rows"
done

python3 - "$OUT/rows" <<'PY'
import math, sys
rows = [tuple(map(float, l.split())) for l in open(sys.argv[1])]
g = math.exp(sum(math.log(c / b) for b, c in rows) / len(rows)) - 1
print(f"\ngeomean over {len(rows)} gems: {g * 100:+.2f}%")
PY
