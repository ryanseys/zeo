# The one entry point for building and verifying zeo. Every recipe is one
# blessed invocation -- the Makefile decides nothing a recipe's own tool does
# not. CI's test legs call the ci-* targets here, so `make gate` and CI run
# the same commands by construction and cannot drift.
#
# `make`            build the workspace (the suites hard-require a fresh
#                   `zeo` binary and `libzeo.a`; nothing gives a test target
#                   a cargo dependency edge to a binary, so build first)
# `make test`       the dev loop: unit + e2e + golden suites, default profile
# `make check`      clippy at CI's severity
# `make gate`       everything: the four CI legs, the whole-gem cases,
#                   the cext surface, doctests, bench
# `make linux`      the Linux container verification loop (needs podman)
#
# Suites spawn compile children and the golden group bounds their memory;
# running two cargo invocations at once defeats that, so this file refuses
# `make -j` reordering.
.NOTPARALLEL:

# Machine-local overrides (e.g. `CARGO = mise x -- cargo`). Not committed.
-include config.mk

CARGO ?= cargo
NEXTEST ?= $(CARGO) nextest run
ZEO_DEV ?= tools/zeo-dev

.PHONY: all test check gate bench install linux clean \
        ci-jit ci-aot ci-leakcheck ci-gccheck ci-doc ci-natlibs ci-anchor

all:
	$(CARGO) build --workspace

test: all
	$(NEXTEST) --workspace

check:
	$(CARGO) clippy --workspace --all-targets --all-features -- -D warnings

# --- The CI legs. ci.yml calls these; `make gate` composes them. -----------

# Cranelift through the in-process JIT, one `zeo` child per golden.
ci-jit: all
	$(NEXTEST) --workspace --no-fail-fast

# The same CLIF through an object file and a real link -- what ships. A
# SMOKE tier, not the whole corpus: JIT and AOT share the emitter, and
# AOT-only bugs have been link/artifact-shaped, which the feature-diverse
# zeo-authored examples plus the e2e suite catch. The full spinel corpus
# takes this leg only in `make gate`.
ci-aot: all
	ZEO_GOLDEN_BACKEND=aot $(NEXTEST) -p zeo --test examples --test gaps --no-fail-fast
	ZEO_E2E_BACKEND=aot $(NEXTEST) -p zeo --test e2e --no-fail-fast

# The compiled-ownership ledger: a non-zero balance at exit is a leak or a
# double-consume in the emitted lowering. Emitted code only, so golden corpora only.
ci-leakcheck: all
	ZEO_RT_LEAKCHECK=1 $(NEXTEST) -p zeo --test examples --test spinel --test gaps --no-fail-fast

# The cycle census: gates CHANGE against each program's `.gccheck` sidecar.
# The whole-gem gemtests leg of this check lives in `gate`, not here.
ci-gccheck: all
	ZEO_GC=1 ZEO_RT_GCCHECK=1 $(NEXTEST) -p zeo --test examples --test spinel --test gaps --no-fail-fast

# nextest doesn't run doctests.
ci-doc: all
	$(CARGO) test --workspace --doc

# The native-library table an AOT link names is HAND-WRITTEN
# (backend/link.rs); this asks rustc for the live answer and diffs it.
# Meaningful on Linux (where `-lcrypt` once went missing while macOS stayed
# green). Ignored by default: it compiles the lib in a probe target dir.
ci-natlibs: all
	$(CARGO) test -p zeo --lib -- --ignored natlibs_table_matches_rustc --nocapture

# `cargo install` ships no runtime archive; an installed zeo builds one on
# first `zeo -o` through an anchor workspace, standing on undocumented
# cargo behaviour. One platform is enough -- the claim is about cargo.
ci-anchor: all
	$(CARGO) test -p zeo --lib -- --ignored a_dependency_position_zeo --nocapture

# --- The phase-boundary gate. ----------------------------------------------

# The gate runs the OS-appropriate one of the two ignored unit tests above.
UNAME := $(shell uname -s)
ifeq ($(UNAME),Darwin)
PLATFORM_CI_LEG := ci-anchor
else
PLATFORM_CI_LEG := ci-natlibs
endif

# Everything: the CI legs, plus the whole-gem cases the default profile opts
# out of (`-P full`), plus gemtests under the cycle census, plus the AOT leg
# over the full spinel corpus (CI runs only the AOT smoke tier), plus bench
# (informational -- bench numbers are recorded, never a gate).
gate: ci-jit ci-aot ci-leakcheck ci-gccheck ci-doc $(PLATFORM_CI_LEG)
	ZEO_GOLDEN_BACKEND=aot $(NEXTEST) -p zeo --test spinel --no-fail-fast
	$(NEXTEST) -p zeo -P full -E 'binary(gemtests) + test(every_bundled_gem_compiles)'
	ZEO_GC=1 ZEO_RT_GCCHECK=1 $(NEXTEST) -p zeo -P full --test gemtests
	$(ZEO_DEV) bench

bench:
	$(ZEO_DEV) bench

install:
	$(CARGO) install --path crates/zeo

linux:
	$(ZEO_DEV) linux

clean:
	$(CARGO) clean
