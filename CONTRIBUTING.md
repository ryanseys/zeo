# Contributing to Zeo

## Setup

- Rust: `rust-toolchain.toml` pins it and rustup installs it. A C compiler
  too, for the libraries a few `-sys` crates vendor and for the link step
  (see "No C, no headers" below).
- Ruby is needed only to RECORD a test's answer (`cargo xtask bless`). The
  version is pinned in `.ruby-version`; point `ZEO_RUBY` at it if it is not
  the `ruby` on PATH. Every ordinary run reads the committed answers instead.
- Network access on a fresh clone, once, to resolve the gems.

```console
$ bundle install      # Gemfile.lock -> vendor/bundle
$ cargo build         # the zeo binary and libzeo.a
```

`Gemfile` and `Gemfile.lock` name every gem version zeo depends on. One lock
decides both what the compiler vendors and what the ruby oracle resolves, so
the two cannot disagree about a version — which they did, and a golden then
recorded the difference as a zeo bug.

The corpus and api suites spawn the built `zeo` binary and link against
`libzeo.a`. There is no build-first ritual: `cargo nextest run` works from a
clean tree. Cargo builds the binary, but a test run never builds the
staticlib — only `cargo build` asks the lib target for every crate-type — so
anything that links an AOT program goes through `runtime_archive()`, which
builds it when it is missing or older than the sources in it. Do not edit a
runtime source while a suite is running: the rebuild re-keys the scratch root
under the run.

## The one rule: oracle-verified, divergence-documented

Zeo's house style is *approximation is fine, silent wrongness is not*:

- New behavior is verified against real `ruby` (the oracle), as a program
  under `test/` in the same change.
- Every intentional divergence gets a comment at the code site explaining
  what differs and why, and user-visible ones also get a row in
  `docs/COMPATIBILITY.md`.
- Error **messages are part of the behavior** — CRuby's exact wording, checked
  by golden output.

## Workflow

Two commands, and CI runs the same two:

```console
$ cargo nextest run          # the dev loop
$ cargo nextest run -P full  # the gate: every leg
$ cargo xtask ci             # every check that is not a test
$ cargo xtask linux          # the container verification loop (needs podman)
```

The default profile is the dev loop: every unit test, the in-process checks,
the api tests, and every corpus program on the JIT plus the AOT smoke tier
really linked. `-P full` adds the legs that re-run the whole corpus (AOT,
memcheck, the zeo-vs-zeo differentials), the whole-graph milestones and the
cases that build a release compiler of their own.

Targeted runs are nextest filters:

```console
$ cargo nextest run -E 'test(core::string/)'   # one area
$ cargo nextest run -E 'test(/^aot_/)'         # the AOT legs
$ cargo xtask bless core::string/              # re-record answers from ruby
$ cargo bench -p zeo --bench programs          # the criterion perf bank
```

- The corpus lives under `test/`: one `.rb` per program, its answer recorded
  under `__END__`, and its DIRECTORY says what it is held to. A green
  `cargo nextest run` is the conformance record. A fixed gap fails as an
  XPASS — promote it with `cargo xtask promote-gap <stem> <topic/area>`.
- `cargo xtask bless` is the only writer of a recorded answer.
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

A program takes both backends, and the leg is a case-name prefix rather than
an environment variable:

```console
$ cargo nextest run -E 'test(core::string/)'      # the JIT
$ cargo nextest run -P full -E 'test(/^aot_core::string/)'  # a real link
```

A third leg is the zeo-vs-zeo differential (`diff_`): every program compiles
and runs three more times -- with `ZEO_DEBUG=no-typed-calls` turning every
TyKind-driven emission off, with packaged ids forced, and down the packaged
cache road -- and every answer must equal the first, byte for byte. Ruby is
not consulted. Run it on any change to typed emission; a wrong static type is
a miscompile, and this leg is what catches one.

`crates/zeo/tests/checks/clif.rs` holds insta snapshots of the emitted CLIF. They
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

### No C, no headers

zeo is Rust. The tree tracks no C, C++ or assembly source and no patch to
one, and no copy of MRI's headers: the C API is `crates/zeo-capi`, Rust over
the runtime, and an extension builds against upstream's headers fetched at
the first build (`docs/EXTENSIONS.md`). Two gates hold the rule.
`crates/zeo/tests/checks/no_c.rs` lists tracked C by extension and holds the
exceptions exact, each dated.
`deny.toml` bans `cc`, `bindgen`, `cmake` and `cxx-build` except through the
crates that compile a vendored library, and the same checks file pins that
set: `onig_sys`, `ruby-prism-sys`, `libffi-sys`, `libmimalloc-sys`,
`openssl-src`/`openssl-sys`, and criterion's `alloca`. `libc` is bindings,
not a build, and is fine anywhere.

To add a crate that compiles C: name it in `deny.toml`'s `wrappers` and in
`no_c.rs`'s `C_COMPILING_CRATES`, with the library it wraps and why a Rust
crate cannot do the job. A test that needs an extension writes the C from a
string constant; it never tracks a `.c`.

## Commits

Small and focused; present-tense summary line; the body says *why*. The full
gate (fmt + clippy + nextest + bench) runs at review boundaries — per-commit,
run the focused checks for what you touched.
