# Contributing to Zeo

## Setup

- Rust ≥ 1.94 (`rust-version` in `Cargo.toml`; `mise.toml` pins the toolchain
  used in development) and a C compiler.
- A real Ruby matching the oracle version pinned in `mise.toml`
  (via `mise install`) — only needed when re-blessing golden output from the
  oracle (`tools/zeo-dev bless`); the committed snapshots cover ordinary runs.

```console
$ make     # cargo build --workspace
```

The golden and e2e suites spawn the built `zeo` binary and link against
`libzeo.a`. Both are products of the `zeo` package — and the suites live in
`crates/zeo/tests/`, so cargo rebuilds both before any suite runs. There is
no "build first" ritual: `cargo nextest run -p zeo --test examples` after a
compiler edit tests the edited compiler.

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

The Makefile is the front door, and it is what CI runs -- the test legs in
`.github/workflows/ci.yml` call the same `ci-*` targets `make gate` composes,
so the gate and CI cannot drift:

```console
$ make test    # the dev loop: unit + e2e + all golden suites (default profile)
$ make check   # clippy at CI's severity
$ make gate    # everything: the CI legs + whole-gem cases + full-corpus AOT + bench
$ make linux   # the container verification loop (needs podman)
```

Targeted runs go through nextest directly:

```console
$ cargo nextest run -p zeo --test spinel      # the full ruby-oracle corpus
$ cargo nextest run -p zeo --test examples --test gaps
$ cargo nextest run -p zeo -P full            # + the whole-gem cases
$ tools/zeo-dev bless <filter>              # re-record goldens from ruby
$ make bench                                # the criterion perf bank (bench/README.md)
$ tools/zeo-dev size                        # what each class table costs a binary
```

The default profile is the dev loop. `-P full` adds the cases that compile a
whole gem's require graph (`gemtests`, `every_bundled_gem_compiles`); they
belong to `make gate`, not to every run.

- The golden suites live under `tests/` (examples + the spinel corpus + the
  XFAIL gaps tracker) and run as datatest-stable `cargo test`/nextest targets;
  a green `cargo nextest` is the conformance record. A fixed gap fails CI as an
  XPASS — promote it with `tools/zeo-dev promote-gap`, which moves it into
  `tests/`, the zeo-authored suite. Not `tests/spinel/`, which mirrors the
  vendored spinel corpus (see `tests/spinel/UPSTREAM.md`).
- `tools/zeo-dev bless` is the only golden writer: `golden.rs` honours
  `ZEO_BLESS_FROM_TOOL`, which only `tools/zeo-dev bless` sets, so a bare
  `ZEO_BLESS=1 cargo test` does nothing.
- Perf-sensitive changes report their bench delta: save a criterion baseline
  before the change (`cargo bench -p zeo --bench programs -- --save-baseline
  before`) and compare after (`critcmp`; see `bench/README.md`).
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
| `zeo -o app file.rb` | **aot** (default) | writes an object file and links it against `libzeo.a` |

There was a second, older backend: `crates/zeo/src/codegen/` emitted Rust
source text and handed it to `rustc`. It was the only backend until Cranelift
reached parity, then the differential oracle during the bring-up, and it was
retired on 2026-08-21. The branch `archive/rustc-backend` keeps it readable.
The oracle for correctness is `ruby` on `PATH`.

The golden suites take two legs, and the e2e suite does too:

```console
$ cargo nextest run -p zeo --test examples                      # jit
$ ZEO_GOLDEN_BACKEND=aot   cargo nextest run -p zeo --test examples
$ cargo nextest run -p zeo --test e2e                           # jit child (default)
$ ZEO_E2E_BACKEND=aot      cargo nextest run -p zeo --test e2e  # link per test
```

A third leg is the typed differential oracle (`make ci-typed`, or
`ZEO_GOLDEN_DIFF_TYPED=1` on any golden suite): every case compiles and
runs twice -- once as-is, once with `ZEO_DEBUG=no-typed-calls` turning
every TyKind-driven emission off -- and the two zeo outputs must agree
byte-for-byte. Run it on any change to typed emission; a wrong static
type is a miscompile, and this leg is what catches one.

`crates/zeo/tests/clif.rs` holds insta snapshots of the emitted CLIF. They
record emitter *shape*, which no golden can see, so **run `cargo nextest run
-p zeo` after any `clif/` change**. Each snapshot is its own `#[test]`, so
one run reports every stale one.

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
