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
| Oracle authority under bless | `tools/zeo-dev bless "pilot::"` records the oracle (zeo for `.divergence`). The blessed `sort_with_comparator.snap` body is **byte-identical** to its committed `.expected` — bless kept zeo's own divergent output. |
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

- **Byte fidelity.** A `.snap` body is a Rust `String`; the pilot decodes
  output with `from_utf8_lossy`. The encoding corpus asserts **invalid
  UTF-8 byte sequences** in goldens today — a lossy decode would corrupt
  exactly the cases the encoding work exists to pin. A bulk migration
  would need escaping machinery insta does not natively give.
- **No new tool surface.** `--unreferenced` and snapshot review need the
  `cargo-insta` binary — not installed, not pinned in `mise.toml`, and
  its review/accept workflow is precisely what the bless guard must keep
  disarmed.
- **Diff noise.** Each `.snap` carries a YAML header + section markers;
  4,392 goldens would each grow boilerplate lines, and the one-time
  migration diff touches every golden file in the repo.
- **LOC.** insta absorbed only "store + diff one string": the pilot ADDS
  ~190 lines (harness tail + engine plumbing) while deleting nothing —
  all sidecar/bless/watchdog/normalization machinery stays custom in
  either format.

## Recommendation

Do **not** bulk-migrate. The engine — the part that was ever expensive —
is shared either way; insta's real wins (inline diffs, orphan checks) are
small against a whole-corpus format churn, a hard UTF-8 fidelity hazard,
and a new tool in the loop that the bless discipline must actively fence.
`tests/spinel/` was staying `.expected` regardless, which would leave the
repo running two formats forever. Keep `.expected`; delete the pilot, or
keep it as a living reference.
