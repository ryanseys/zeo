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
#   scripts/linux/verify.sh -E '<expr>'   # one nextest filter, for triage
#
# Every stage tees its output to `logs/<stage>.log` beside this script.
# The container runs `--rm`, so its `podman logs` die with it -- a run whose
# output only existed there could not be read back at all.
#
# Every stage needs `build` to have run first (nothing rebuilds the `zeo`
# BINARY for a test target). The target volume persists between runs, so a
# second `build` is incremental.
#
# The container runs as ROOT, and one golden can tell: `process_identity_rows`
# expects `Errno::EPERM` from `Sys.setuid("root")` and its siblings, which
# succeed here. CI and any developer machine run unprivileged, so the
# expectation is right and this leg is the odd one out -- do not bless it away.
set -euo pipefail

# RELEASE, not dev. Every AOT golden LINKS a whole binary against
# `libzeo.a`, and the dev archive is 315 MB against release's 87 MB --
# measured, a hello compile+link is 1.48s dev and 0.69s release, and this
# leg pays that 4,061 times. `ZEO_CLIF_VERIFY=1` keeps what
# `debug_assertions` was buying here: the Cranelift verifier and the
# ownership ledger.
PROFILE=${ZEO_LINUX_PROFILE:-release}
if [ "$PROFILE" = release ]; then
  CARGO_PROFILE=--release
  TARGET_DIR=release
else
  # cargo's dev profile writes to `debug/`, which is why the two names
  # cannot be the same variable.
  CARGO_PROFILE=
  TARGET_DIR=debug
fi
export ZEO_CLIF_VERIFY=1

IMAGE=${ZEO_LINUX_IMAGE:-zeo-linux}
# The VM's own width. The e2e tier LINKS a whole binary per test, so this
# run is bound by `cc` far more than by the compiler -- threads are the
# lever that matters, and the ceiling is the podman machine's cpu count
# (`podman machine set --cpus N`, which needs the machine stopped).
THREADS=${ZEO_LINUX_THREADS:-8}
MEMORY=${ZEO_LINUX_MEMORY:-10g}
VOLUME=${ZEO_LINUX_VOLUME:-zeo-linux-target}
ENGINE=${ZEO_CONTAINER_ENGINE:-podman}
REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)

LOGS="$REPO/scripts/linux/logs"

run() {
  # --platform: see the Dockerfile header. --memory: the golden harness's
  # own RSS watchdog assumes room to work.
  #
  # The repo mounts READ-WRITE, and that is deliberate rather than lazy: a
  # golden runs with its cwd set to `tests/`, and a dozen of them create a
  # temp file there on purpose (`File.write("test_size_temp.bin", ..)`) --
  # the oracle did exactly that when the `.expected` was blessed. A
  # read-only mount turns every one of those into `Errno::EROFS`, which
  # reads as a zeo bug and is not one.
  #
  # --pids-limit: podman defaults to 2048, and the thread goldens spawn
  # enough OS threads at eight-way parallelism to hit it -- `pthread_create`
  # then fails EAGAIN and the runtime panics mid-test. Another failure that
  # is the harness, not the program.
  #
  # -it only when there IS a terminal: the same script runs from a
  # non-tty caller (CI, an agent), where podman refuses the flag.
  local tty=(); [ -t 0 ] && [ -t 1 ] && tty=(-it)
  "$ENGINE" run --rm "${tty[@]}" --platform linux/arm64 \
    --memory "$MEMORY" \
    -e ZEO_CLIF_VERIFY=1 \
    --pids-limit 16384 \
    -v "$REPO":/src -v "$VOLUME":/target -v "$LOGS":/logs \
    -w /src "$IMAGE" bash -c "$1" 2>&1 | tee "$LOGS/$STAGE.log"
  return "${PIPESTATUS[0]}"
}

stage() {
  STAGE=$(printf '%s' "$1" | tr -cs 'A-Za-z0-9_.-' '_')
  mkdir -p "$LOGS"
  echo "=== linux: $1  (log: scripts/linux/logs/$STAGE.log)"
  case "$1" in
    build)   run "cargo build --workspace $CARGO_PROFILE" ;;
    # No env var = the default (jit) leg, four threads: the goldens spawn a
    # child zeo each and the watchdog caps them at 512 MiB.
    jit)     run "cargo nextest run $CARGO_PROFILE -p zeo-tests --test-threads $THREADS --no-fail-fast" ;;
    aot)     run "ZEO_GOLDEN_BACKEND=aot cargo nextest run $CARGO_PROFILE -p zeo-tests --test-threads $THREADS --no-fail-fast --test examples --test spinel --test gaps" ;;
    units)   run "cargo nextest run $CARGO_PROFILE -p zeo -p zeo-rt --test-threads $THREADS" ;;
    # The one test that asks rustc for the live answer instead of trusting
    # the table; ignored by default because it compiles the lib in a probe
    # target dir. This is the platform whose table was never confirmed.
    natlibs) run 'cargo test -p zeo --lib -- --ignored natlibs_table_matches_rustc --nocapture' ;;
    valgrind) run 'set -e
      cd /tmp && printf "%s\n" "class P; def initialize(n) = @n = n; def to_s = \"P(#{@n})\"; end" \
        "10.times { |i| puts P.new(i) }" "a = (1..50).map { |i| i * i }; puts a.sum" \
        "begin; raise ArgumentError, \"x\"; rescue => e; puts e.message; end" > vg.rb
      /target/$TARGET_DIR/zeo vg.rb -o vg
      valgrind --error-exitcode=9 --leak-check=full --errors-for-leak-kinds=definite ./vg' ;;
    cross)   run 'rustup target add x86_64-unknown-linux-gnu
      CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
      CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \
      AR_x86_64_unknown_linux_gnu=x86_64-linux-gnu-ar \
      cargo check --workspace --target x86_64-unknown-linux-gnu' ;;
    shell)   run 'exec bash' ;;
    # Triage: one nextest filter expression against the default profile.
    # `verify.sh -E 'test(foo)'` after a red run, instead of the hour the
    # whole corpus costs.
    -E)      run "cargo nextest run $CARGO_PROFILE -p zeo-tests --test-threads $THREADS --no-fail-fast -E '$FILTER'" ;;
    *)       echo "unknown stage: $1" >&2; exit 2 ;;
  esac
}

if [ "${1:-}" = -E ]; then
  FILTER=${2:?"-E needs a nextest filter expression"}
  stage -E
  exit
fi
if [ $# -eq 0 ] || [ "$1" = all ]; then
  set -- build jit aot units natlibs valgrind
fi
for s in "$@"; do stage "$s"; done
