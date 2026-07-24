# Conformance gaps (expected-to-fail)

Ruby programs zeo does **not yet** match `ruby` on — checked in and tracked so
we can grind them down over time. Same file format as
[`../spinel/`](../spinel) (`<name>.rb` + `<name>.rb.expected` stdout golden,
plus optional `.err.expected` / `.args` / `.stdin` sidecars), so promoting a
fixed gap is a plain move into the corpus.

Some gaps are stderr-only divergences on output zeo intentionally won't
reproduce (ruby's experimental-API / duplicate-key warnings, thread exception
dumps) — those stay parked here rather than in the passing suite.

## The XFAIL contract

Each gap runs through `crates/zeo/tests/gaps.rs` (a `cargo test`/nextest target)
in **`Mode::Xfail`**:

- A gap that still **diverges** from its golden → the test **PASSES** (the
  known failure is still there — expected).
- A gap that starts **matching** ruby → the test **FAILS** with a "GAP FIXED —
  promote" message. Promote it into the zeo-authored suite (`tests/`) with:

  ```sh
  scripts/promote-gap.sh foo   # moves foo.rb + sidecars to tests/, verifies it
  ```

  Promote to **`tests/`**, not `tests/spinel/` — that dir mirrors the vendored
  spinel corpus, and a spinel-origin gap re-promotes on its own the next time
  `scripts/import-spinel-corpus.sh` triages it.

## Goldens are recorded from ruby, never hand-written

The `.expected` is the **ruby 4.0.5 oracle** output (the target zeo must
eventually produce). Record/refresh it with:

```sh
ZEO_BLESS=1 cargo test -p zeo --test gaps          # all gaps
ZEO_BLESS=1 cargo test -p zeo --test gaps -- foo   # one gap
```

## Adding a gap

Drop in `foo.rb`, then `ZEO_BLESS=1 cargo test -p zeo --test gaps -- foo` to
capture its golden. If zeo already matches ruby, the test will tell you it's not
a gap — put it in the corpus instead. New gaps usually arrive via
`scripts/import-spinel-corpus.sh` (spinel triage).
