#!/usr/bin/env bash
# Run zeo's suites on Linux, in the container `scripts/linux/Dockerfile`
# builds. The stand-in for a CI leg, and the only place the Linux link path
# is exercised at all (see the Dockerfile's header for why that matters).
#
#   scripts/linux/verify.sh build     # cargo build --workspace
#   scripts/linux/verify.sh jit       # the default golden legs
#   scripts/linux/verify.sh aot       # the same corpora, linked binaries
#   scripts/linux/verify.sh units     # zeo + zeo-rt unit suites
#   scripts/linux/verify.sh natlibs   # diff link.rs's glibc table vs rustc
#   scripts/linux/verify.sh valgrind  # leak-check a linked program
#   scripts/linux/verify.sh cross     # x86_64 compile check
#   scripts/linux/verify.sh shell     # an interactive prompt in the image
#   scripts/linux/verify.sh all       # build jit aot units natlibs valgrind
#
# Every stage needs `build` to have run first (nothing rebuilds the `zeo`
# BINARY for a test target). The target volume persists between runs, so a
# second `build` is incremental.
set -euo pipefail

IMAGE=${ZEO_LINUX_IMAGE:-zeo-linux}
VOLUME=${ZEO_LINUX_VOLUME:-zeo-linux-target}
ENGINE=${ZEO_CONTAINER_ENGINE:-podman}
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)

run() {
  # --platform: see the Dockerfile header. --memory: the golden harness's
  # own RSS watchdog assumes room to work; 6 GiB matches what the macOS
  # side has spare. Repo read-only -- a test that writes into the source
  # tree is a bug, and this is where it gets caught.
  # -it only when there IS a terminal: the same script runs from a
  # non-tty caller (CI, an agent), where podman refuses the flag.
  local tty=(); [ -t 0 ] && [ -t 1 ] && tty=(-it)
  "$ENGINE" run --rm "${tty[@]}" --platform linux/arm64 \
    --memory 6g \
    -v "$REPO":/src:ro -v "$VOLUME":/target \
    -w /src "$IMAGE" bash -c "$1"
}

stage() {
  echo "=== linux: $1"
  case "$1" in
    build)   run 'cargo build --workspace' ;;
    # No env var = the default (jit) leg, four threads: the goldens spawn a
    # child zeo each and the watchdog caps them at 512 MiB.
    jit)     run 'cargo nextest run -p zeo-tests --test-threads 4 --no-fail-fast' ;;
    aot)     run 'ZEO_GOLDEN_BACKEND=aot cargo nextest run -p zeo-tests --test-threads 4 --no-fail-fast --test examples --test spinel --test gaps' ;;
    units)   run 'cargo nextest run -p zeo -p zeo-rt --test-threads 4' ;;
    # The one test that asks rustc for the live answer instead of trusting
    # the table; ignored by default because it compiles the lib in a probe
    # target dir. This is the platform whose table was never confirmed.
    natlibs) run 'cargo test -p zeo --lib -- --ignored natlibs_table_matches_rustc --nocapture' ;;
    valgrind) run 'set -e
      cd /tmp && printf "%s\n" "class P; def initialize(n) = @n = n; def to_s = \"P(#{@n})\"; end" \
        "10.times { |i| puts P.new(i) }" "a = (1..50).map { |i| i * i }; puts a.sum" \
        "begin; raise ArgumentError, \"x\"; rescue => e; puts e.message; end" > vg.rb
      /target/debug/zeo vg.rb -o vg
      valgrind --error-exitcode=9 --leak-check=full --errors-for-leak-kinds=definite ./vg' ;;
    cross)   run 'rustup target add x86_64-unknown-linux-gnu
      CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
      CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \
      AR_x86_64_unknown_linux_gnu=x86_64-linux-gnu-ar \
      cargo check --workspace --target x86_64-unknown-linux-gnu' ;;
    shell)   run 'exec bash' ;;
    *)       echo "unknown stage: $1" >&2; exit 2 ;;
  esac
}

if [ $# -eq 0 ] || [ "$1" = all ]; then
  set -- build jit aot units natlibs valgrind
fi
for s in "$@"; do stage "$s"; done
