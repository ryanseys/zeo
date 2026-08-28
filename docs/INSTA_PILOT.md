# The insta pilot: `.snap` vs `.expected`, measured

Overhaul decision 2: before any bulk golden-format migration, run ~10
representative goldens under insta `.snap` storage and compare. The pilot
lives in `tests/insta-pilot/` (10 cases duplicated from `tests/` — their
originals still run, so coverage is unchanged) with the harness tail in
`crates/zeo/tests/insta_pilot.rs`. Deleting the whole experiment is
`rm -r tests/insta-pilot docs/INSTA_PILOT.md`, one `[[test]]` block, and
the two `pilot_*` functions in `golden.rs`.

The cases cover every sidecar mechanism: plain stdout (`hello`, `blocks`,
`unary_operators`), stderr + nonzero exit (`a_bignum_index…`,
`an_unrescued_definition_raise…`), `.divergence` (`sort_with_comparator`,
`a_computed_require_of_a_bundled_gem`), `.macos-only`
(`errno_full_surface`), `.gc` + `.gccheck` (`a_cycle_is_reclaimed`), and a
case with no snapshot at all (the live-oracle fallback).

## What was verified

| Claim | Result |
|---|---|
| Same engine, both formats | `pilot_run`/`pilot_reference` reuse sidecars, skips, the spawned child, normalization, and the census gate; only the store-and-diff tail differs. 10/10 green. |
| Oracle authority under bless | `cargo xtask bless "pilot::"` records the oracle (zeo for `.divergence`). The blessed `sort_with_comparator.snap` body is **byte-identical** to its committed `.expected` — bless kept zeo's own divergent output. |
| The `cargo insta accept` hole is closed | `INSTA_UPDATE` set without `ZEO_BLESS_FROM_TOOL` panics naming the bless command (verified); ordinary runs force `INSTA_UPDATE=no`. |
| Census stays out of the snapshot | `a_cycle_is_reclaimed` gates its `.gccheck` sidecar exactly as before; the snapshot holds only program output. |
| Live-oracle fallback | The no-snapshot case runs ruby live and compares, like a fresh `.expected`-less golden. |
| Wall clock | Indistinguishable — the child run dominates; the pilot's 10 cases run in ~0.3s each either way. |

## Where insta wins

- **Failure UX.** An induced diff renders as one inline unified diff
  (`-ok` / `+Hello, world?`) instead of the four-section
  expected/actual stdout/stderr dump.
- **Orphan detection.** `cargo insta test --unreferenced=reject` can fail
  on a `.snap` no test references. `.expected` has no equivalent —
  though `goldens_hygiene.rs` covers the adjacent risks (sidecar naming,
  divergence↔expected consistency) and would still be needed.
- **One file per case** (stdout and stderr in one body, stderr section
  absent = must-be-empty preserved) instead of up to two.

## Where `.expected` wins

- **Byte fidelity.** A `.snap` body is a Rust `String` and the pilot
  decodes output with `from_utf8_lossy`. **Measured 2026-08-24: zero of
  the 4,395 committed `.expected` files contain invalid UTF-8 today**, so
  this is a future-hazard needing an escape hatch (a case kept on
  `.expected`, or insta's experimental `assert_binary_snapshot!`), not a
  present blocker. The sharper present risk is **trailing-newline
  leniency**: exactly 3 goldens pin output with no trailing newline
  (`stdout_survives_*` — the AOT stdout-flush bug class), and insta's
  text comparison must be verified byte-strict there or the body format
  must end with an explicit terminator line.
- **The vendored corpus.** `tests/spinel/` (3,010 cases, 69% of the
  corpus) vendors upstream's own `.rb.expected` files and the sync diffs
  them — migrating it breaks upstream diffability, so it stays
  `.expected` under any adoption: two formats along the
  zeo-authored/vendored boundary.
- **Orphan detection is not free under nextest.** `--unreferenced`
  requires running the suite through `cargo insta test`; CI runs
  nextest, so the practical orphan check is a few lines in
  `goldens_hygiene.rs` — available to either format.
- **The authority model.** insta's headline ergonomics (`cargo insta
  review`/`accept`) let a HUMAN decide truth; zeo's goldens let RUBY
  decide. Review/accept must stay fenced forever, so adoption uses insta
  as storage + diff rendering only.
- **LOC.** insta absorbed only "store + diff one string": the pilot ADDS
  ~190 lines while deleting nothing — sidecar/bless/watchdog/
  normalization machinery stays custom in either format.

## Deep-dive addendum (2026-08-24, insta.rs docs + corpus measurement)

Alternatives examined and rejected:

- **`insta::glob!` instead of datatest-stable**: glob runs the whole
  corpus inside ONE `#[test]`, forfeiting nextest's per-case processes —
  and with them the 60s per-case deadline, the per-child RSS watchdog,
  the golden-group width, and per-case reporting. Non-negotiable losses
  for a 4,392-case corpus with runaway-child history.
- **Inline Ruby in `.rs` tests** (the e2e shape, applied to the corpus):
  the corpus stops being runnable under plain `ruby`/`zeo` — the probe
  workflow used daily — and bless becomes a Rust-source rewriter at
  4,392-case scale. The e2e tier is where hand-authored assertions
  belong; oracle-recorded output is not that.
- **Single-file format (`__END__`/DATA golden inside the `.rb`)**: max
  file reduction (one file per case), stays ruby-runnable, byte-exact —
  but bespoke beyond even `.expected` (zero external tooling), collides
  with programs that use `DATA` themselves (1 today), and appends zeo
  data to every vendored spinel body, wrecking upstream diffs.
- **Magic-comment sidecars** (`#@ args:` in the `.rb`): only 70 sidecar
  files exist besides the goldens (6 `.args`, 1 `.stdin`, 11
  `.divergence`, 34 `.gccheck`, 8 `.macos-only`, …); folding them in
  saves little and adds a directive parser plus spinel sync noise.

What migration would actually buy, measured against the pilot: one
co-located `.snap` per case instead of `.expected` (+ sometimes
`.err.expected`), insta's inline unified diff on failure, and a format
editors highlight. Costs: a ~2,800-file mechanical commit, a permanent
two-format repo (spinel), the trailing-newline verification/mitigation,
and cargo-insta as fenced-but-present tooling.
