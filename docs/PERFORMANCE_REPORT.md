# Compilation Performance Report

Why `examples/` and the conformance corpus are slow to compile, run, and test —
and the plan to fix it. All numbers below are measured on this machine (arm64
macOS, 12 cores), not estimated.

## Executive summary

A full conformance run (`cargo run -p xtask conformance run --dir …/spinel/test`,
1968 tests) takes **19m31s**. Essentially all of that is **per-program `rustc`**:
the compiler itself (parse → analyze → codegen) is 0.14s per program, the Ruby
oracle is not in the hot path, and running the compiled binaries is negligible
(40ms mean).

Two independent costs dominate, and both are large:

1. **Code volume** — every generated program embeds an identical ~8,000-line
   exception prelude + factory that `rustc` recompiles from scratch ~1,765 times.
2. **Fixed link overhead** — the default macOS linker against the 20 MB runtime
   dylib, plus ad-hoc codesigning, costs ~5.9s even for a *one-line* program.

The lowest-risk, config-only fix is switching `rustc`'s linker to **`lld`**. In
isolation it cut a full compile from 7–8s to ~3s, but a controlled A/B under the
harness's real 12-way saturation showed only **~1.3×** — because when every core
is already busy with rustc *codegen* of the identical prelude, the linker stops
being the bottleneck. That pointed at the decisive fix: moving the ~8,000-line
exception prelude out of every program and into the precompiled runtime, which
removes the CPU-bound codegen that dominates under saturation.

**Both landed.** `puts 1` went from **8,671 → 587 generated lines**, and mean
per-program compile time on a fixed 119-test subset (cold, `-j 12`, verdicts
identical throughout) fell **8,437 ms → 2,169 ms — a 3.9× speedup**:

| stage | mean compile_ms | subset wall |
|---|---|---|
| baseline (default linker) | 8,437 | 89.7 s |
| + `lld` | 6,386 | 69.6 s |
| + Stage A (factory → runtime) | 4,667 | 55.1 s |
| **+ Stage B+C (native prelude)** | **2,169** | **42.8 s** |

Extrapolated to the full 1968-test corpus (baseline ~19.5 min), this puts a cold
run near **~5–6 min**. Verified: all 645 e2e tests and 419 runtime/compiler unit
tests pass; the conformance subset's PASS/FAIL verdicts are unchanged.

## The pipeline (per test)

`spinelc <src> -o <bin>` runs as a subprocess per case (harness: 12 workers,
`crates/xtask/src/conformance/runner.rs:106-127`). Inside, `spinelc` invokes **one
`rustc`** (`crates/spinelc/src/build.rs:177`) against the prebuilt `spinel-rt`
dylib — not a throwaway Cargo project. `spinel-rt` is built **once** up front
(`runner.rs:316`), then reused for every test via `--extern spinel_rt=<lib>` and
`-L dependency=target/debug/deps`.

The Ruby oracle is **not** in the hot path: all 1962 top-level tests ship
`.expected` snapshots, so `ruby` runs only for the (≈0) snapshot-less cases, and
its results are content-addressed and cached. Comparing output is a disk read.

So per test: `spinelc` (fast) → `rustc` (slow) → run binary (fast) → compare.

## Where the time goes (measured)

Aggregated from the 1968 result stamps (`target/conformance/spinel/stamps`):

- **compile_ms: total 14,097s of CPU, mean 7,163 ms/program, max 24,479 ms.**
- **run_ms: total 79s, mean 40 ms.** Running the binaries is negligible.

Isolated single-compile breakdown of `puts 1` (which generates **8,671 lines** of
Rust):

| stage | time |
|---|---|
| spinelc parse + analyze + codegen (in-process) | **0.14 s** |
| rustc, default macOS linker, dynamic link | **7–8 s** |
| rustc, **`lld` linker**, dynamic link | **~3.0 s** |
| rustc CPU only (user + sys) | ~2.0 s |
| trivial 1-line program, default linker | 5.9 s |
| trivial 1-line program, **`lld` linker** | 2.4 s |

### Cost 1 — code volume: the exception prelude

Every generated program embeds the identical built-in exception hierarchy
(`EXCEPTION_PRELUDE`, `crates/spinelc/src/parse/mod.rs:44-173`), spliced into every
program's HIR. Although 24 of ~28 classes are empty in Ruby (`class TypeError <
StandardError; end`), they are not empty once generated: `analyze::mro::materialize`
copies every inherited method onto each subclass and `collect_ivars` flattens
inherited ivars, so each class emits full `initialize`/`message`/`to_s`/`inspect`/
`full_message`/`backtrace` bodies plus dispatch trampolines as a
`spinel_rt::ruby_class!` invocation.

Measured on the current `puts 1` output (8,671 lines):

| chunk | lines | share |
|---|---|---|
| 45 `ruby_class!` prelude class blocks (lines 3–6462) | ~6,600 | ~76% |
| exception factory inside `fn main` (lines 7473–8607) | ~1,130 | ~13% |
| the actual program (`kernel_puts`) | **1** | — |

`rustc` re-parses, re-typechecks, and re-codegens this ~99%-identical block on
every one of ~1,765 programs. (This is documented in
`docs/todo/runtime-exception-model.md`, which recorded it at 2,296 lines/program;
the prelude has since grown to 8,671.)

### Cost 2 — fixed per-invocation overhead: linking + codesigning

A *one-line* program that merely links the runtime still costs **5.9s** with the
default linker, of which only ~0.7s is CPU. The rest is:

- the default macOS linker (`ld`) linking against the **20 MB
  `libspinel_rt.dylib`**,
- `rustc` reading the 47 MB rlib / 20 MB dylib metadata at startup,
- macOS ad-hoc codesigning + Gatekeeper of every output binary.

Switching to **`lld`** drops the same one-line program to **2.4s** and `puts 1`
from 7–8s to ~3.0s *on an idle machine*. The lld-linked binary was verified to run
and produce correct output.

**But that 2.5× is an idle-machine figure.** A controlled same-session A/B on a
119-test subset (`--filter 'a*'`, cold, `-j 12`, verdicts identical both runs)
tells the real story under the harness's 12-way saturation:

| linker | wall | mean compile_ms | max |
|---|---|---|---|
| default | 89.7 s | 8,437 ms | 12,117 ms |
| **`lld`** | **69.6 s** | **6,386 ms** | 10,920 ms |

So in the actual harness `lld` is worth **~1.3×**, not 2.5× — because when all 12
cores are already busy with rustc *codegen* of the identical prelude, the linker's
mostly-serial-I/O cost overlaps other cores' work and stops being the bottleneck.
`lld` is still free, deterministic, and a real ~1.3× on the single biggest time
sink, so it is worth keeping. **But the A/B proves the dominant cost under
saturation is CPU-bound prelude codegen, which only Step 3 removes.**

## Why every development run is cold (the multiplier)

- The binary cache generation key = `fnv1a64(linkage + spinel-rt lib len:mtime)`
  (`build.rs:325-336`).
- The stamp toolchain fingerprint = spinelc + rlib mtime/len + `ruby -v`
  (`crates/xtask/src/conformance/stamps.rs:25-37`).

So **any rebuild of `spinelc` or `spinel-rt` invalidates the entire binary cache
and all 1968 stamps at once**, and the whole corpus recompiles cold. This is
correct — a compiler/runtime change can change any result — but it means that
during active development every run pays the full 19.5 minutes. Warm re-runs with
no toolchain change replay from stamps in seconds; that path already works.

## Contributing factors

- **No optimization or linker flags anywhere.** No `[profile.*]` sections;
  generated programs compile at `rustc` defaults (opt-level 0, codegen-units 16,
  debuginfo 0). `-C incremental` is deliberately off (`build.rs:423-432`, a
  measured net loss under libtest's per-thread execution).
- **12-way oversubscription.** 12 harness workers, each spawning a `rustc` that
  itself uses up to 16 codegen threads, on 12 physical cores. (`codegen-units=1`
  measured no faster for these small files — not worth pursuing.)
- **The runtime pulls heavy dependencies** (`default = ["ext-all"]`): two regex
  engines (`regex` + `fancy-regex`), `num-bigint`, Unicode tables, `may`,
  `serde_json`, `yaml-rust2`, RustCrypto hashes, socket, openssl. These inflate the
  dylib/rlib that every program links against and whose metadata `rustc` reads.

## Plan

### Step 1 — `lld` linker (config-only, ~2.5×)

Add `-C link-arg=-fuse-ld=lld` to the `rustc` invocation in `build.rs` when linking
dynamically (the harness path), detecting lld once and falling back silently to the
default linker if it is absent. Measured 7–8s → ~3s per program; projected corpus
wall time ~19.5 min → **~8 min**. Files: `crates/spinelc/src/build.rs`.

### Step 2 — slim the harness runtime (investigated, NOT worth it)

The idea: build `spinel-rt` for the harness with only the `ext-*` features the
corpus needs (measured union: `json`, `base64`, `stringio`, `openssl`). Measured
the actual payoff before implementing:

| runtime build | dylib size |
|---|---|
| `ext-all` (current) | 21.19 MB |
| slim (corpus union) | 19.36 MB (−8.6%) |
| bare (no exts) | 18.27 MB (−13.8%) |

**Dropped.** The saving is only ~1.8 MB because the heavy dependencies —
`regex` + `fancy-regex`, `num-bigint`, the Unicode tables, `may` — are **not**
ext-gated and dominate the dylib; the only exts that pull external crates are
`digest`, `json`, `psych`, and the corpus needs `json`. An 8.6%-smaller dylib
shaves only tens of ms/program off the metadata-read/link cost, which under
saturation is not the bottleneck anyway. Against that, forcing non-default
features makes Cargo rebuild the whole 32k-line runtime every time a developer
alternates between `cargo test` (default features) and `conformance run` (slim) —
a net loss. The real runtime weight is the always-on deps, and making those
optional is a much larger, correctness-sensitive change, not a quick win.

### Step 3 — move the exception machinery into `spinel-rt` (staged)

The structural fix, done in three independently-verifiable stages so each can be
measured and shipped on its own.

**Stage A — move the exception factory to the runtime (DONE, measured).** Every
generated program used to install a ~1,130-line closure mapping each raisable
exception class name to an inline `emit_boxed_new` constructor, because
`spinel-rt` "could not construct exception objects itself." But each `ruby_class!`
already registers a `__construct` `ConstructorFn`, and the registry already stores
each class's fully-qualified name — so the runtime can construct by name directly.
`ClassRegistry` gained a `by_name` index and a `construct_exception(name, msg)`
method; `raise_error`, `coerce_raise_arg`, and `raise_stop_iteration` now call it,
and codegen stops emitting the factory. Files: `crates/spinel-rt/src/dispatch.rs`,
`crates/spinelc/src/codegen/mod.rs`.

Measured on the `a*` subset (cold, `-j 12`, verdicts identical to baseline):

| config | mean compile_ms | subset wall |
|---|---|---|
| default linker (baseline) | 8,437 | 89.7 s |
| + `lld` | 6,386 | 69.6 s |
| **+ `lld` + Stage A** | **4,667** | **55.1 s** |

`puts 1` dropped from 8,671 → 7,497 generated lines. Stage A alone cut compile 27%
(the factory's dense construction code costs more than its 13% line share), for
~1.81× cumulative so far.

**Stage B — pin prelude ClassIds (DONE).** The prelude classes are registered
first (from `EXCEPTION_PRELUDE`, in source order) and so already get deterministic
ids `63..`; the one exception was `Math::DomainError`, registered *after* the
whole statement loop, so its id floated with the user-class count. `analyze` now
registers it via `register_math_domain_error` right after the prelude and before
any user class, so its id is a fixed function of the prelude alone (verified: id
108 whether the program has 0 or 3 user classes; previously 108 vs 111). That is
the property Stage C needs — a precompiled prelude can only agree with codegen on
which id each class is if those ids don't depend on user code. (A stronger version
records the ids in `spinel-abi` under the contiguity assert; deferred as a
robustness guard to land with Stage C's regeneration path.) File:
`crates/spinelc/src/analyze/mod.rs`.

**Stage C — hand-written native prelude in `spinel-rt` (DONE, measured).** The
built-in exception hierarchy is now written once, natively, in the runtime:

- `spinel-abi` gains `EXCEPTION_PRELUDE_CLASSES`, the shared source of truth for
  the 46 classes' ids (63–108, contiguous, asserted), names, and superclass links.
- `spinel-rt`'s `crate::prelude` defines one native `RubyException` type (name-keyed
  ivars, real frozen/dup/reflection) backing every exception class, plus
  `register_prelude(&mut ClassRegistry)` which installs all 46 with linearized
  ancestors, a shared `ConstructorFn`, and the six shared `Exception` methods
  (`initialize`/`message`/`to_s`/`backtrace`/`full_message`/`inspect`) + StopIteration's
  two. `ConstructorFn` gained a leading `ClassId` so one constructor backs all.
- `spinelc` stops emitting the prelude entirely: `is_bootstrap` now excludes those
  classes from all three emission loops, `main` calls `spinel_rt::register_prelude`,
  a bootstrap class's `New`/raise routes through `construct_by_class_id` (its
  static type is now `Poly`, no struct named), and `analyze` asserts its assigned
  ids match `spinel-abi`. The prelude HIR stays for name resolution, `super`
  inlining, and materializing user subclasses — so `class MyError < StandardError`
  is unchanged.

**Result: `puts 1` went from 8,671 → 587 generated lines (−93%).** A cold single
compile (dynamic + `lld`) dropped to **1.81 s** (from ~3 s at Stage A, ~7–8 s at
baseline), with rustc CPU down to ~0.5 s (from ~2 s) — exactly the CPU-bound
prelude codegen the saturation A/B pinpointed. Files:
`crates/spinel-abi/src/lib.rs`, `crates/spinel-rt/src/prelude.rs`,
`crates/spinel-rt/src/dispatch.rs`, `crates/spinelc/src/codegen/mod.rs`,
`crates/spinelc/src/types.rs`, `crates/spinelc/src/codegen/call.rs`,
`crates/spinelc/src/analyze/mod.rs`.

The native prelude also unblocks the object-model work the deferred doc lists
(`Exception#cause`, `Ractor` deep-copy, name-keyed `instance_variable_set` on
exceptions — the last now actually works).

**Known limitation.** REOPENING a built-in exception class to add methods
(`class StandardError; def shout; …; end`) no longer takes effect — the class is
no longer emitted, so a user-added method isn't registered on it, and the call
raises `NoMethodError` at runtime. `docs/todo/runtime-exception-model.md` already
flagged this as rare and untested (its blocker #5), and it holds across the full
e2e suite and the conformance subset (no test exercises it). Supporting it later
means emitting a targeted `define_method` registration for just the user-added
methods on the bootstrap class id — a small, additive follow-up.

### Deferred (revisit after measuring Steps 1–3)

- **cranelift** codegen backend for harness compiles (nightly is installed; 2–3×
  faster debug codegen on throwaway binaries).
- **Leased `-C incremental` dir pool** per worker (largely subsumed by Step 3).
- **Reachable-class prelude pruning** (partial alternative to Step 3, unneeded if
  Step 3 lands).

## Harness observability

To make per-test cost (and coverage) legible while iterating, the conformance
runner now prints:

- **Inline per-test timing** — each verdict line carries `(c:<compile>ms
  r:<run>ms)`, so a single slow case stands out in the stream instead of just
  making the run feel "bursty" (fast cache hits interleaved with cold `rustc`
  compiles). `compile` is `spinelc` + `rustc`; `run` is the compiled program.
- **A "top 15 slowest tests" table** at the end (total / compile / run), to pick
  out a pathological program — a large generated file (high `compile`) or a
  nearly-hanging run (high `run`).
- **A pass-rate line** — `PASS RATE <passed>/<total> (<pct>%)`, and the same
  percentage in `conformance/SCOREBOARD.md`, so coverage is one glanceable number
  that should climb over time.

## How to measure changes

- Per-program: `time rustc … <generated>.rs` before/after, and assert the binary
  still runs and matches the oracle.
- Corpus: run a **fixed ~100-test subset** through the harness before/after and
  compare wall time and PASS count. Do not re-run the full 19.5-minute suite to
  measure — the subset is representative and the full run is a warm-cache replay
  anyway once toolchain-stable.
- Correctness gate: PASS/FAIL counts must not regress from the 1430-PASS baseline.
