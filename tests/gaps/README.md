# Conformance gaps (expected-to-fail)

Ruby programs zeo does **not yet** match `ruby` on — checked in and tracked so
we can grind them down over time. Same file format as
[`../spinel/`](../spinel) (`<name>.rb` + `<name>.rb.expected` stdout golden,
plus optional `.err.expected` / `.args` / `.stdin` sidecars), so promoting a
fixed gap is a plain move into the corpus.

Every gap is a real divergence with a real cause, and its header comment says
what that cause is.

A gap may be a program that makes the compiler **panic**. The harness contains
a panic and counts it as a divergence, so an internal error can be recorded
here rather than taking the test binary down with it. A `Mode::Pass` failure
names it as a panic, because a bug and a stated limitation want different
work. A divergence zeo has decided not to reproduce does not
belong here — it belongs in a passing test that documents it.

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

  A promoted file's goldens carry its OLD path (`gaps/foo.rb:12`), so re-bless
  it in its new home right after: `ZEO_BLESS=1 cargo test -p zeo --test
  examples -- foo`.

Both stdout **and stderr** are compared, byte-exactly, after the shared
normalization in `crates/zeo/tests/support/golden.rs` (line endings, the
source path, and object addresses — `0x` + 16 hex digits → `0xADDR`, since
those are process-random on both sides).

## Goldens are recorded from ruby, never hand-written

The `.expected` is the **ruby 4.0.6 oracle** output (the target zeo must
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

Keep at least one gap here: `datatest-stable` panics rather than reporting zero
cases, so an empty directory breaks the suite. If the last one is ever fixed,
retire `crates/zeo/tests/gaps.rs` and its `[[test]]` entry along with it.
