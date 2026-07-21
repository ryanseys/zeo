# Contributing to zeo

## Setup

- Rust ≥ 1.87 (`rust-version` in `Cargo.toml`; `mise.toml` pins the toolchain
  used in development) and a C compiler.
- A real Ruby matching the oracle version in `conformance/SCOREBOARD.md`
  (via `mise install`) — only needed when regenerating expected output or
  oracle-verifying; the committed snapshots cover ordinary runs.

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
$ cargo test --workspace                          # unit + e2e
$ cargo run -p xtask -- conformance run --smoke   # quick tier (per-PR CI runs this)
$ cargo run -p xtask -- conformance run           # full corpus (nightly CI)
$ cargo run -p xtask -- conformance triage --bucket <name>   # inspect a failure cluster
$ cargo run -p xtask -- bench                     # perf vs bench/baseline.tsv
```

- The conformance scoreboard (`conformance/SCOREBOARD.md`) must not regress;
  update it with `--update-scoreboard` when your change moves it.
- Perf-sensitive changes report their `xtask bench` delta; intentional shifts
  are banked by committing `--update-baseline`'s diff.
- `cargo fmt --all` and `cargo clippy --workspace --all-targets` (zero
  warnings) gate CI. Don't add `#[allow]`s to dodge lints — fix or discuss.

## Code conventions

- `unwrap()`/`expect()` only for lock acquisition or provably-infallible
  cases (comment why); anything a user program can reach must raise a real,
  rescuable Ruby exception instead of panicking.
- Raise through the typed macros (`type_error!`, `arg_error!`, …); argument
  conversion goes through `builtins/convert.rs` (the `rb_convert_type`
  protocol), arity checks through `arity!`.
- One Ruby class/module per runtime module, methods declared in
  `builtin_methods!` tables with oracle-verified `[arity]` metadata.
- Module docs explain *design rationale*, not narration; keep them current —
  a stale claim is treated as a bug.

## Commits

Small and focused; present-tense summary line; the body says *why*. The full
gate (fmt + clippy + tests + conformance + bench) runs at review boundaries —
per-commit, run the focused checks for what you touched.
