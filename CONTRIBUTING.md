# Contributing to Zeo

## Setup

- Rust ≥ 1.94 (`rust-version` in `Cargo.toml`; `mise.toml` pins the toolchain
  used in development) and a C compiler.
- A real Ruby matching the oracle version pinned in `mise.toml`
  (via `mise install`) — only needed when re-blessing golden output from the
  oracle (`tools/zeo-dev bless`); the committed snapshots cover ordinary runs.

```console
$ cargo build --workspace     # do this first, and after every compiler edit
```

**`cargo build` first, always.** It produces two things the test suites need
and cannot build for themselves: the `zeo` binary the golden harness spawns,
and `libzeo.a`, the runtime archive every compiled program links against.
Nothing gives a test target a cargo dependency edge to a *binary* target, so a
stale `zeo` would otherwise run yesterday's compiler over today's goldens and
report green. The harness stats the compiler's sources against the binary and
refuses instead — if a whole suite fails with *"the zeo CLI … is older than
…"*, that is this, and `cargo build` is the fix. (`cargo nextest run
--workspace` rebuilds it for you; `cargo nextest run -p zeo-tests` does not.)

## The one rule: oracle-verified, divergence-documented

Zeo's house style is *approximation is fine, silent wrongness is not*:

- New behavior is verified against real `ruby` (the oracle), ideally as a
  conformance test or e2e case in the same change.
- Every intentional divergence gets a comment at the code site explaining
  what differs and why, and user-visible ones also get a row in
  `docs/COMPATIBILITY.md`.
- Error **messages are part of the behavior** — CRuby's exact wording, checked
  by golden output.

## Workflow

```console
$ cargo nextest run --workspace                     # unit + e2e + all golden suites
$ cargo nextest run -p zeo-tests --test spinel      # the full ruby-oracle corpus
$ cargo nextest run -p zeo-tests --test examples --test gaps
$ cargo nextest run -p zeo-tests -P full            # + the whole-gem cases
$ tools/zeo-dev bless <filter>              # re-record goldens from ruby
$ tools/zeo-dev bench                       # perf vs bench/baseline.tsv
$ tools/zeo-dev size                        # what each class table costs a binary
```

The default profile is the dev loop. `-P full` adds the cases that compile a
whole gem's require graph (`gemtests`, `every_bundled_gem_compiles`); they
belong to a phase gate, not to every run.

- The golden suites live under `tests/` (examples + the spinel corpus + the
  XFAIL gaps tracker) and run as datatest-stable `cargo test`/nextest targets;
  a green `cargo nextest` is the conformance record. A fixed gap fails CI as an
  XPASS — promote it with `tools/zeo-dev promote-gap`, which moves it into
  `tests/`, the zeo-authored suite. Not `tests/spinel/`, which mirrors the
  vendored spinel corpus (see `tests/spinel/UPSTREAM.md`).
- `tools/zeo-dev bless` is the only golden writer: `golden.rs` honours
  `ZEO_BLESS_FROM_TOOL`, which only `tools/zeo-dev bless` sets, so a bare
  `ZEO_BLESS=1 cargo test` does nothing.
- Perf-sensitive changes report their `tools/zeo-dev bench` delta; intentional shifts
  are banked by committing `--update-baseline`'s diff.
- `cargo clippy --workspace --all-targets` at zero warnings gates CI. Don't
  add `#[allow]`s to dodge lints — fix or discuss.
- The tree is rustfmt-clean, but CI does **not** gate it. `cargo fmt` is safe
  to run and should be a no-op; keep your change to it small, and don't
  reformat code you didn't touch.

## The two backends

Zeo lowers Ruby to Cranelift IR and emits machine code itself. That is the
compiler; `crates/zeo/src/clif/` is where it lives.

| Spelling | Backend | What it does |
|---|---|---|
| `zeo file.rb`, `zeo -e` | **jit** (default) | finalizes the CLIF into this process and runs it in place |
| `zeo file.rb -o app` | **aot** (default) | writes an object file and links it against `libzeo.a` |

There was a second, older backend: `crates/zeo/src/codegen/` emitted Rust
source text and handed it to `rustc`. It was the only backend until Cranelift
reached parity, then the differential oracle during the bring-up, and it was
retired on 2026-08-21. The branch `archive/rustc-backend` keeps it readable.
The oracle for correctness is `ruby` on `PATH`.

The golden suites take two legs:

```console
$ cargo nextest run -p zeo-tests --test examples                      # jit
$ ZEO_GOLDEN_BACKEND=aot   cargo nextest run -p zeo-tests --test examples
```

`crates/zeo/tests/clif.rs` holds insta snapshots of the emitted CLIF. They
record emitter *shape*, which no golden can see, so **run `cargo nextest run
-p zeo` after any `clif/` change**. insta stops at the first failing snapshot,
so "1 failed" does not mean "1 stale" — fix and re-run until it is quiet.

## Code conventions

- `unwrap()`/`expect()` only for lock acquisition or provably-infallible
  cases (comment why); anything a user program can reach must raise a real,
  rescuable Ruby exception instead of panicking.
- Raise through the typed macros (`type_error!`, `arg_error!`, …); argument
  conversion goes through `builtins/convert.rs` (the `rb_convert_type`
  protocol).
- One Ruby class/module per runtime module, declared with the
  `ruby_class!`/`ruby_module!` DSL (a second class in the same file needs its
  own inline `mod`: each block emits one `lookup`). A def's parameter list is
  the ONLY place its shape is written — it gives both the argument-count check
  and its `Method#arity`. Never write an arity number: the one exception is a
  `|`-joined name that genuinely differs from its def (`"<<" arity 1 | "push"`),
  and a test fails any override that merely restates the parameter list. Add
  `cfunc` when CRuby declares the method `argc = -1`, which discards a
  signature the DSL can still express. The arity oracle that gated this
  reached zero and is retired, so a disagreement with ruby is now found by a
  golden, not a ledger.
- Declare a method on the class CRuby owns it on — that decides which receivers
  answer it, so `IO#flock` (File's) and `Module#superclass` (Class's) were
  behaviour bugs, not reflection details. Write a golden that calls the
  method on a receiver only the correct owner gives — that is what catches a
  wrong class or an invented name now.
- Module docs explain *design rationale*, not narration; keep them current —
  a stale claim is treated as a bug.

## Commits

Small and focused; present-tense summary line; the body says *why*. The full
gate (fmt + clippy + nextest + bench) runs at review boundaries — per-commit,
run the focused checks for what you touched.
