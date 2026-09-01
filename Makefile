# The one entry point for building and verifying zeo. Every recipe is one
# blessed invocation -- the Makefile decides nothing a recipe's own tool does
# not. CI calls these same targets, so a workflow and a local run cannot
# drift.
#
# A target is named for the QUESTION IT ASKS, never for who calls it. The
# legs were `ci-*` for as long as CI was their only caller; `check-batch` and
# `gate` compose them locally too, so the name had stopped being true.
#
# `make help` lists everything, generated from the `##` comments below. The
# block that used to live here was hand-maintained and had already drifted.
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

# Every target here is a verb, not a file. One list, so a new target cannot
# be half-declared: the old one omitted `ci-features` and `ci-size`, and a
# file of either name would have turned that target into a silent no-op.
.PHONY: all help deps test test-jit test-aot test-memcheck \
        test-typed test-packaged test-milestones test-config test-platform test-size \
        test-all lint check-batch gate bench pgo install linux clean

.DEFAULT_GOAL := all

all: deps  ## build the workspace
	$(CARGO) build --workspace

# Two columns, read off the targets themselves, which is the whole point.
help:  ## list every target
	@grep -hE '^[a-z][a-z-]*:.*?##' $(MAKEFILE_LIST) | sort | \
	  awk 'BEGIN {FS = ":.*?## "}; {printf "  %-16s %s\n", $$1, $$2}'

# Resolve `Gemfile.lock` into `vendor/bundle`. One committed lock decides both
# what the compiler vendors and what the ruby oracle resolves, so the two
# cannot disagree about a version. Needs the network the first time and
# nothing after it. `.bundle/config` sets the path and refuses to rewrite the
# lock during an install; changing the Gemfile means `bundle lock` on purpose.
deps:  ## resolve Gemfile.lock into vendor/bundle (needs network once)
	@$(BUNDLE) install --quiet

# --- The dev loop. ---------------------------------------------------------

test: all  ## the dev loop: unit + e2e + golden suites, stops at the first failure
	$(NEXTEST) --workspace

lint:  ## clippy at CI's severity
	$(CARGO) clippy --workspace --all-targets --all-features -- -D warnings

# --- The verification legs. `check-batch` and `gate` compose them. ----------

# The same command as `test`, run to the end. Two names because the questions
# differ: a dev wants the first failure, a batch wants the whole list.
test-jit: all  ## every suite through the in-process JIT, to the end
	$(NEXTEST) --workspace --no-fail-fast

# The same CLIF through an object file and a real link -- what ships. A
# SMOKE tier, not the whole corpus: JIT and AOT share the emitter, and
# AOT-only bugs have been link/artifact-shaped, which the feature-diverse
# zeo-authored examples plus the e2e suite catch. The full spinel corpus
# takes this leg only in `make gate`.
test-aot: all  ## the AOT smoke tier: goldens minus spinel, plus e2e, really linked
	ZEO_GOLDEN_BACKEND=aot $(NEXTEST) -p zeo --no-fail-fast -E 'binary(goldens) - test(spinel::)'
	ZEO_E2E_BACKEND=aot $(NEXTEST) -p zeo --test e2e --no-fail-fast

# ONE instrumented corpus pass carrying both memory checks -- the
# compiled-ownership ledger (a non-zero balance at exit is a leak or a
# double-consume in the emitted lowering) AND the cycle census (gates
# CHANGE against each program's `.gccheck` sidecar). The two compose:
# verified 4,393/4,393 with both armed, 2026-08-25. Emitted code only,
# so golden corpora only. The whole-gem compile lives in `gate`.
test-memcheck: all  ## one corpus pass under the ownership ledger and the cycle census
	ZEO_RT_LEAKCHECK=1 ZEO_GC=1 ZEO_RT_GCCHECK=1 $(NEXTEST) -p zeo --test goldens --no-fail-fast

# The typed differential oracle: every golden compiles and runs TWICE --
# once as-is, once with every TyKind-driven emission off
# (ZEO_DEBUG=no-typed-calls) -- and the two zeo outputs must agree
# byte-for-byte. Ruby is not consulted; this catches a typed fold
# changing ANY observable behavior, including behavior the CRuby oracle
# could not distinguish. Run it on any change to typed emission.
test-typed: all  ## every golden twice, with typed emission on and off, diffed
	ZEO_GOLDEN_DIFF_TYPED=1 $(NEXTEST) -p zeo --test goldens --no-fail-fast

# The packaged-codegen differential (separate compilation, decision 11):
# every golden compiles and runs TWICE -- once as-is, once with the Packaged
# id mode forced program-wide over an identity table
# (ZEO_DEBUG=packaged-ids) -- and the two zeo outputs must agree
# byte-for-byte. At M2 scope this proves the packaged CODEGEN (the id-table
# loads and the variable patched-bit guard) over the whole corpus; when M6
# compiles gems as packages, the leg widens to true spliced-vs-packaged.
test-packaged: all  ## every golden twice, with packaged id codegen on and off, diffed
	ZEO_GOLDEN_DIFF_PKGIDS=1 $(NEXTEST) -p zeo --test goldens --no-fail-fast

# The umbrella entry points, one named case each (tests/milestones/). Each
# splices a whole library's require graph, so this is minutes rather than
# seconds and the default profile opts the binary out; it gets its own leg
# instead of slowing the dev loop. A `pending/` case is an XFAIL and says so
# the day it starts matching ruby.
test-milestones: all  ## the umbrella entry points, one whole require graph each
	$(NEXTEST) -p zeo -P full --no-fail-fast -E 'test(milestone::)'

# Two questions about the SHAPES the crates promise to build in, neither of
# which runs the corpus. They were separate legs only because they arrived
# separately.
#
# Doctests, because nextest does not run them.
#
# Then: every `ext-*` feature really is optional. Two configurations are
# enough -- the empty set is the strictest, and the docs.rs set is the one
# that ships (it omits `ext-ffi` and `ext-openssl`, both of which build
# vendored C). Each has silently stopped compiling once. `check`, not
# `build`: this asks whether the cfgs are right, not for an artifact.
DOCS_RS_FEATURES := $(shell sed -n '/\[package.metadata.docs.rs\]/,/^\[dependencies\]/p' \
	crates/zeo-rt/Cargo.toml | sed -n 's/^ *"\(ext-[a-z0-9]*\)",*/\1/p' | paste -sd, -)
test-config: all  ## doctests, and that every ext-* feature is really optional
	$(CARGO) test --workspace --doc
	$(CARGO) check -p zeo-rt --no-default-features
	$(CARGO) check -p zeo-rt --no-default-features --features '$(DOCS_RS_FEATURES)'

# One target, whichever of the two ignored unit tests this OS can answer.
# Both ask a question about the platform's own toolchain, so they are one
# leg wearing two hats rather than two legs.
#
# Linux -- the native-library table an AOT link names is HAND-WRITTEN
# (backend/link.rs); this asks rustc for the live answer and diffs it. This
# is where `-lcrypt` once went missing while macOS stayed green.
#
# macOS -- `cargo install` ships no runtime archive, so an installed zeo
# builds one on first `zeo -o` through an anchor workspace, standing on
# undocumented cargo behaviour. One platform is enough: the claim is about
# cargo, not about the OS.
UNAME := $(shell uname -s)
ifeq ($(UNAME),Darwin)
PLATFORM_TEST := a_dependency_position_zeo
else
PLATFORM_TEST := natlibs_table_matches_rustc
endif

test-platform: all  ## the one platform claim this OS can answer (natlibs | anchor)
	$(CARGO) test -p zeo --lib -- --ignored $(PLATFORM_TEST) --nocapture

# `puts 1` is the floor every program pays, and a table joining the always-on
# set moves it for every program at once. Its own target, and its own CI job:
# it builds the release compiler and links a program.
test-size:  ## the binary-size floor (builds the release compiler)
	$(NEXTEST) -p zeo --test checks --run-ignored all -E 'test(binary_size::)'

# --- The composed tiers. ---------------------------------------------------
#
# NOTHING RUNS TWICE IN ONE CONFIGURATION, and two facts keep it that way.
# Both are filters, so an edit to either can break this silently -- re-check
# with `cargo nextest list` after touching one.
#
#   1. The three whole-gem names are excluded by `[profile.default]`'s
#      default-filter in `.config/nextest.toml`, so `--workspace` cannot
#      reach them and the `-P full` legs own them alone.
#      Measured: workspace 7175, milestones 10, whole-gem 1, no overlap.
#
#   2. `test-aot` and `gate`'s spinel line PARTITION the goldens binary --
#      `- test(spinel::)` and `& test(spinel::)`.
#      Measured: 1565 + 3131 = 4696 = every case, none twice, none missed.
#
# A golden DOES run under several legs -- jit, then AOT, then instrumented,
# then typed. That is four different questions about one program, not one
# question asked four times.

# The mid-tier for batch boundaries INSIDE a phase: cheaper than the gate,
# broader than `make test`. One workspace pass, the AOT smoke tier, one
# instrumented corpus pass.
check-batch: test-jit test-aot test-memcheck  ## the mid-tier between batches (~4 min)

# The boundary: every leg, plus the whole-gem compile the default profile
# opts out of (`-P full`) and the AOT leg over the full spinel corpus (CI
# runs only the AOT smoke tier).
# Bench is deliberately NOT here: perf numbers are recorded on their own
# cadence (`make bench` after perf commits and at re-banks), never gated.
gate: test-jit test-aot test-memcheck test-config test-milestones test-platform  ## the phase boundary: every leg, plus full-spinel AOT
	ZEO_GOLDEN_BACKEND=aot $(NEXTEST) -p zeo --no-fail-fast -E 'binary(goldens) & test(spinel::)'
	$(NEXTEST) -p zeo -P full -E 'test(every_bundled_gem_compiles)'

# Everything, with no judgement about when it is worth running. `gate` is a
# CURATED boundary -- it leaves out the legs whose cost only pays back
# against a particular change, and that curation is deliberate:
#
#   test-typed     every golden twice; it answers a question about TyKind
#                  emission, so it earns its minutes on a typed change
#   test-packaged  every golden twice; it answers a question about packaged
#                  id codegen, and CI runs it on every push anyway
#   test-size      builds the release compiler and links a program
#
# Use this before a release, or when you want the answer rather than the
# fastest sufficient answer. `make linux` is NOT here: it needs podman and
# runs a different platform, so it is its own thing.
test-all: gate test-typed test-packaged test-size  ## every test target, no exceptions (slowest)

# --- Measurement, packaging, platforms. ------------------------------------

bench:  ## the criterion bench bank
	$(CARGO) bench -p zeo --bench programs

# The shipped-configuration bank: the same corpus timed against a full
# `cargo xtask dist --pgo` snapshot (~15 min of setup). Release-boundary
# measurements only; never rewrites the committed bench/results.tsv --
# compare runs with --save-baseline + critcmp.
pgo:  ## the bench bank against a PGO dist build (release boundaries)
	ZEO_BENCH_DIST=pgo $(CARGO) bench -p zeo --bench programs

install:  ## cargo install this working tree
	$(CARGO) install --path crates/zeo

linux:  ## the Linux container verification loop (needs podman)
	$(CARGO) xtask linux

clean:  ## cargo clean
	$(CARGO) clean
