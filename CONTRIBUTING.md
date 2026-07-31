# Contributing to zeo

## Setup

- Rust ≥ 1.87 (`rust-version` in `Cargo.toml`; `mise.toml` pins the toolchain
  used in development) and a C compiler.
- A real Ruby matching the oracle version pinned in `mise.toml`
  (via `mise install`) — only needed when re-blessing golden output from the
  oracle (`ZEO_BLESS=1`); the committed snapshots cover ordinary runs.

## The one rule: oracle-verified, divergence-documented

zeo's house style is *approximation is fine, silent wrongness is not*:

- New behavior is verified against real `ruby` (the oracle), ideally as a
  conformance test or e2e case in the same change.
- Every intentional divergence gets a comment at the code site explaining
  what differs and why, and user-visible ones also get a row in
  `docs/COMPATIBILITY.md`.
- Error **messages are part of the behavior** — CRuby's exact wording, checked
  by golden output.

## Workflow

```console
$ cargo nextest run --workspace                   # unit + e2e + all golden suites
$ cargo nextest run -p zeo --test spinel          # the full ruby-oracle corpus
$ cargo nextest run -p zeo --test examples --test gaps
$ ZEO_BLESS=1 cargo test -p zeo --test <suite>    # re-record goldens from ruby
$ cargo run -p xtask -- bench                     # perf vs bench/baseline.tsv
```

- The golden suites live under `tests/` (examples + the spinel corpus + the
  XFAIL gaps tracker) and run as datatest-stable `cargo test`/nextest targets;
  a green `cargo nextest` is the conformance record. A fixed gap fails CI as an
  XPASS — promote it into `tests/spinel/`.
- Perf-sensitive changes report their `xtask bench` delta; intentional shifts
  are banked by committing `--update-baseline`'s diff.
- `cargo clippy --workspace --all-targets` at zero warnings gates CI. Don't
  add `#[allow]`s to dodge lints — fix or discuss.
- The tree is rustfmt-clean, but CI does **not** gate it. `cargo fmt` is safe
  to run and should be a no-op; keep your change to it small, and don't
  reformat code you didn't touch.

## Code conventions

- `unwrap()`/`expect()` only for lock acquisition or provably-infallible
  cases (comment why); anything a user program can reach must raise a real,
  rescuable Ruby exception instead of panicking.
- Raise through the typed macros (`type_error!`, `arg_error!`, …); argument
  conversion goes through `builtins/convert.rs` (the `rb_convert_type`
  protocol).
- One Ruby class/module per runtime module, declared with the
  `ruby_class!`/`ruby_module!` DSL. A def's parameter list gives both its
  argument-count check and its `Method#arity`; `cargo run -p xtask --
  arity-oracle` records what ruby reports and the `builtin_arity` test gates it.
- Module docs explain *design rationale*, not narration; keep them current —
  a stale claim is treated as a bug.

## Commits

Small and focused; present-tense summary line; the body says *why*. The full
gate (fmt + clippy + nextest + bench) runs at review boundaries — per-commit,
run the focused checks for what you touched.
