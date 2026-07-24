# Conformance gaps (expected-to-fail)

Ruby programs zeo does **not yet** match `ruby` on — checked in and tracked so
we can grind them down over time. Same file format as
[`../corpus/test/`](../corpus/test) (`<name>.rb` + `<name>.rb.expected` stdout
golden, plus optional `.err.expected` / `.args` / `.stdin` sidecars), so
promoting a fixed gap is a plain `git mv` into the corpus.

## The XFAIL contract

Each gap runs through `crates/zeo/tests/gaps.rs` (a `cargo test`/nextest target)
in **`Mode::Xfail`**:

- A gap that still **diverges** from its golden → the test **PASSES** (the
  known failure is still there — expected).
- A gap that starts **matching** ruby → the test **FAILS** with a "GAP FIXED —
  promote" message. That's the signal to move it into the corpus:

  ```sh
  git mv conformance/gaps/foo.rb* conformance/corpus/test/
  # if foo's output embeds its source path, re-bless under its new home:
  ZEO_BLESS=1 cargo test -p zeo --test corpus -- foo
  ```

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
`cargo xtask conformance import` (spinel triage).
