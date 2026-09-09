# Contributing to Zeo

Issues and pull requests are welcome.

[Getting started](docs/tutorials/getting-started.md) is the setup: clone,
`cargo xtask deps`, `cargo build`, `cargo nextest run`. Ruby is needed only
to record a test's answer.

The tools the loop needs, beyond the pinned Rust toolchain and a C compiler:
`cargo-nextest` (every suite), `cargo-deny` and `cargo-machete` (`cargo
xtask check`), `critcmp` (comparing benchmark baselines), and `podman` for
the optional Linux loop. `cargo install` each; CI pins the versions in
`.github/workflows/ci.yml`.

## The one rule: oracle-verified, divergence-documented

The house style is *approximation is fine, silent wrongness is not.*

1. New behaviour is verified against real `ruby`, as a program under `test/`
   in the same change.
2. Every intentional divergence gets a comment at the code site saying what
   differs and why.
3. A divergence a user can observe also gets a row in
   [Compatibility](docs/reference/compatibility.md) and, when it is not
   deliberate, a program under [`test/gaps/`](test/gaps).

## The loop

```console
$ cargo nextest run          # the dev loop and the corpus gate
$ cargo nextest run -P full  # adds the tests that are slow one at a time
$ cargo xtask check             # every check that is not a test
$ cargo xtask linux          # the container verification loop (needs podman)
```

[Run the tests](docs/how-to/run-the-tests.md) has the filters.
[Add a test](docs/how-to/add-a-test.md) and
[Record an answer](docs/how-to/record-an-answer.md) have the rest.

Two things worth knowing before your first run:

- **Do not edit a runtime source while a suite is running.** The rebuild
  re-keys the scratch root under the run, and the failures that follow point
  everywhere except at the cause.
- **`cargo xtask check` builds zeo with five other feature sets.** They go to
  `target/ci-features` so they cannot leave the wrong binary where the suites
  look, but a `cargo build` afterwards is still the safe habit.

## What gates a change

- `cargo nextest run` green.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` at
  zero warnings, which is what `cargo xtask check` runs. Do not add
  `#[allow]`s to dodge a lint — fix it or discuss it.
- Formatting is **not** gated, and the tree is not rustfmt-clean: about a
  hundred files predate the habit. Keep formatting to the code you touched.
  `cargo fmt` reformats the whole module tree from any file you hand it,
  which buries a real change in noise.
- Performance is **not** a gate. A perf claim needs a fresh measurement
  beside it — see [Measure performance](docs/how-to/measure-performance.md).

## Code conventions

- `unwrap()`/`expect()` only for lock acquisition or provably-infallible
  cases (comment why); anything a user program can reach must raise a real,
  rescuable Ruby exception instead of panicking.
- Raise through the typed macros (`type_error!`, `arg_error!`, …); argument
  conversion goes through `builtins/support/convert.rs` (the `rb_convert_type`
  protocol).
- One Ruby class/module per runtime module, declared with the
  `ruby_class!`/`ruby_module!` DSL (a second class in the same file needs its
  own inline `mod`: each block emits one `lookup`). A def's parameter list is
  the ONLY place its shape is written — it gives both the argument-count check
  and its `Method#arity`. Never write an arity number: the one exception is a
  `|`-joined name that genuinely differs from its def (`"<<" arity 1 | "push"`),
  and a test fails any override that merely restates the parameter list. Add
  `cfunc` when CRuby declares the method `argc = -1`, which discards a
  signature the DSL can still express. A disagreement with ruby is found by
  a golden, not a declared table.
- Declare a method on the class CRuby owns it on — that decides which receivers
  answer it: `flock` belongs to File, not IO, and `superclass` to Class, not
  Module. Write a golden that calls the method on a receiver only the
  correct owner gives — that is what catches a wrong class or an invented
  name.
- Module docs explain *design rationale*, not narration; keep them current —
  a stale claim is treated as a bug.

### No C, no headers

zeo is Rust. The tree tracks no C, C++ or assembly source and no patch to
one, and no copy of MRI's headers: the C API is `crates/zeo-capi`, Rust over
the runtime, and an extension builds against upstream's headers fetched at
the first build ([Build a C-extension gem](docs/how-to/build-a-c-extension-gem.md)).
The one exception is `crates/zeo-capi/csrc/`: three files of variadic entry
points (`rb_raise`, `rb_funcall`, `rb_scan_args`) that Rust cannot write
until `c_variadic` stabilises. Two gates hold the rule.
`crates/zeo/tests/suite/checks/no_c.rs` lists tracked C by extension and
holds that exception exact.
`deny.toml` bans `cc`, `bindgen`, `cmake` and `cxx-build` except through the
crates that compile a vendored library, and the same checks file pins that
set: `onig_sys`, `ruby-prism-sys`, `libffi-sys`, `libmimalloc-sys`,
`openssl-src`/`openssl-sys`, `zeo-capi` (for `csrc/`), and criterion's
`alloca`. `libc` is bindings,
not a build, and is fine anywhere.

To add a crate that compiles C: name it in `deny.toml`'s `wrappers` and in
`no_c.rs`'s `C_COMPILING_CRATES`, with the library it wraps and why a Rust
crate cannot do the job. A test that needs an extension writes the C from a
string constant; it never tracks a `.c`.

## Commits

Small and focused. Present-tense summary line; the body says *why*, in plain
sentences. Run the focused checks for what you touched per commit, and the
full gate at a review boundary.
