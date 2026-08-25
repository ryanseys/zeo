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
        ci-jit ci-aot ci-leakcheck ci-gccheck ci-cext ci-doc

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

# The same CLIF through an object file and a real link -- what ships.
# Scoped to the golden corpora: it links a binary per case.
ci-aot: all
	ZEO_GOLDEN_BACKEND=aot $(NEXTEST) -p zeo-tests --test examples --test spinel --test gaps --no-fail-fast

# The compiled-ownership ledger: a non-zero balance at exit is a leak or a
# double-consume in the emitted lowering. Emitted code only, so golden corpora only.
ci-leakcheck: all
	ZEO_RT_LEAKCHECK=1 $(NEXTEST) -p zeo-tests --test examples --test spinel --test gaps --no-fail-fast

# The cycle census: gates CHANGE against each program's `.gccheck` sidecar.
# The whole-gem gemtests leg of this check lives in `gate`, not here.
ci-gccheck: all
	ZEO_GC=1 ZEO_RT_GCCHECK=1 $(NEXTEST) -p zeo-tests --test examples --test spinel --test gaps --no-fail-fast

# The C extension surface is off by default and dead-strips; the workspace
# leg cannot see its tests.
ci-cext: all
	$(NEXTEST) -p zeo-rt --features cext

# nextest doesn't run doctests.
ci-doc: all
	$(CARGO) test --workspace --doc

# --- The phase-boundary gate. ----------------------------------------------

# Everything: the CI legs, plus the whole-gem cases the default profile opts
# out of (`-P full`), plus gemtests under the cycle census, plus bench
# (informational -- bench numbers are recorded, never a gate).
gate: ci-jit ci-aot ci-leakcheck ci-gccheck ci-cext ci-doc
	$(NEXTEST) -p zeo-tests -P full -E 'binary(gemtests) + test(every_bundled_gem_compiles)'
	ZEO_GC=1 ZEO_RT_GCCHECK=1 $(NEXTEST) -p zeo-tests -P full --test gemtests
	$(ZEO_DEV) bench

bench:
	$(ZEO_DEV) bench

install:
	$(CARGO) install --path crates/zeo

linux:
	$(ZEO_DEV) linux

clean:
	$(CARGO) clean
