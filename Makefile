# The one entry point for building and verifying zeo. Every recipe is one
# blessed invocation -- the Makefile decides nothing a recipe's own tool does
# not. CI's test legs call the ci-* targets here, so `make gate` and CI run
# the same commands by construction and cannot drift.
#
# `make install-deps`
#                   resolve Gemfile.lock into vendor/bundle (needs network
#                   once); the gem set both the compiler and the oracle read
# `make`            build the workspace
# `make test`       the dev loop: unit + e2e + golden suites, default profile
# `make check`      clippy at CI's severity
# `make check-batch` the mid-tier between batches inside a phase: the
#                   workspace suites, the AOT smoke tier, and one
#                   instrumented corpus pass (~4 min)
# `make gate`       the boundary: every CI leg, the whole-gem cases,
#                   doctests, the milestone entry points, and the
#                   full-spinel AOT corpus. Bench is NOT in the gate --
#                   numbers are recorded on their own cadence
#                   (`make bench`), never a gate
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
BUNDLE ?= bundle

.PHONY: all test check check-batch gate bench pgo install linux clean ci-typed \
        ci-jit ci-aot ci-memcheck ci-doc ci-natlibs ci-anchor ci-milestones \
        install-deps

all: install-deps
	$(CARGO) build --workspace

# Resolve `Gemfile.lock` into `vendor/bundle`. One committed lock decides both
# what the compiler vendors and what the ruby oracle resolves, so the two
# cannot disagree about a version. Needs the network the first time and
# nothing after it. `.bundle/config` sets the path and refuses to rewrite the
# lock during an install; changing the Gemfile means `bundle lock` on purpose.
install-deps:
	@$(BUNDLE) install --quiet

test: all
	$(NEXTEST) --workspace

check:
	$(CARGO) clippy --workspace --all-targets --all-features -- -D warnings

# The mid-tier for batch boundaries INSIDE a phase: cheaper than the gate,
# broader than `make test`. One workspace pass, the AOT smoke tier, one
# instrumented corpus pass.
check-batch: ci-jit ci-aot ci-memcheck

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
	ZEO_GOLDEN_BACKEND=aot $(NEXTEST) -p zeo --no-fail-fast -E 'binary(goldens) - test(spinel::)'
	ZEO_E2E_BACKEND=aot $(NEXTEST) -p zeo --test e2e --no-fail-fast

# ONE instrumented corpus pass carrying both memory checks -- the
# compiled-ownership ledger (a non-zero balance at exit is a leak or a
# double-consume in the emitted lowering) AND the cycle census (gates
# CHANGE against each program's `.gccheck` sidecar). The two compose:
# verified 4,393/4,393 with both armed, 2026-08-25. Emitted code only,
# so golden corpora only. The whole-gem gemtests half lives in `gate`.
ci-memcheck: all
	ZEO_RT_LEAKCHECK=1 ZEO_GC=1 ZEO_RT_GCCHECK=1 $(NEXTEST) -p zeo --test goldens --no-fail-fast

# The typed differential oracle: every golden compiles and runs TWICE --
# once as-is, once with every TyKind-driven emission off
# (ZEO_DEBUG=no-typed-calls) -- and the two zeo outputs must agree
# byte-for-byte. Ruby is not consulted; this catches a typed fold
# changing ANY observable behavior, including behavior the CRuby oracle
# could not distinguish. Run it on any change to typed emission.
ci-typed: all
	ZEO_GOLDEN_DIFF_TYPED=1 $(NEXTEST) -p zeo --test goldens --no-fail-fast

# The umbrella entry points, one named case each (tests/milestones/). Each
# splices a whole library's require graph, so this is minutes rather than
# seconds and the default profile opts the binary out; it gets its own leg
# instead of slowing the dev loop. A `pending/` case is an XFAIL and says so
# the day it starts matching ruby.
ci-milestones: all
	$(NEXTEST) -p zeo -P full --no-fail-fast -E 'test(milestone::)'

# nextest doesn't run doctests.
ci-doc: all
	$(CARGO) test --workspace --doc

# Every `ext-*` feature really is optional. Two configurations are enough:
# the empty set is the strictest, and the docs.rs set is the one that ships
# -- it omits `ext-ffi` and `ext-openssl` (both build vendored C), and it
# silently stopped compiling once. `check`, not `build`: this asks whether
# the cfgs are right, not for an artifact.
DOCS_RS_FEATURES := $(shell sed -n '/\[package.metadata.docs.rs\]/,/^\[dependencies\]/p' \
	crates/zeo-rt/Cargo.toml | sed -n 's/^ *"\(ext-[a-z0-9]*\)",*/\1/p' | paste -sd, -)
ci-features:
	$(CARGO) check -p zeo-rt --no-default-features
	$(CARGO) check -p zeo-rt --no-default-features --features '$(DOCS_RS_FEATURES)'

# `puts 1` is the floor every program pays, and a table joining the always-on
# set moves it for every program at once. Ignored by default: it builds the
# release compiler and links a program.
ci-size:
	$(NEXTEST) -p zeo --test checks --run-ignored all -E 'test(binary_size::)'

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

# The boundary gate: the CI legs, the whole-gem cases the default profile
# opts out of (`-P full`), gemtests under both memory checks, and the AOT
# leg over the full spinel corpus (CI runs only the AOT smoke tier).
# Bench is deliberately NOT here: perf numbers are recorded on their own
# cadence (`make bench` after perf commits and at re-banks), never gated.
gate: ci-jit ci-aot ci-memcheck ci-doc ci-milestones ci-features $(PLATFORM_CI_LEG)
	ZEO_GOLDEN_BACKEND=aot $(NEXTEST) -p zeo --no-fail-fast -E 'binary(goldens) & test(spinel::)'
	$(NEXTEST) -p zeo -P full -E 'test(gemtest::) + test(every_bundled_gem_compiles)'
	ZEO_RT_LEAKCHECK=1 ZEO_GC=1 ZEO_RT_GCCHECK=1 $(NEXTEST) -p zeo -P full -E 'test(gemtest::)'

bench:
	$(CARGO) bench -p zeo --bench programs

# The shipped-configuration bank: the same corpus timed against a full
# `cargo xtask dist --pgo` snapshot (~15 min of setup). Release-boundary
# measurements only; never rewrites the committed bench/results.tsv --
# compare runs with --save-baseline + critcmp.
pgo:
	ZEO_BENCH_DIST=pgo $(CARGO) bench -p zeo --bench programs

install:
	$(CARGO) install --path crates/zeo

linux:
	$(CARGO) xtask linux

clean:
	$(CARGO) clean
