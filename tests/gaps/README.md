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
work.

A divergence zeo has decided not to reproduce **and has matched deliberately
differently** belongs in a passing test that documents the choice. One it has
decided not to reproduce **because matching would cost more than the
divergence does** also belongs in a passing test, and gets one — see
[Decided divergences](#decided-divergences-live-in-tests-not-here) below.

## Two kinds of file live here

Read a gap's header for which one it is; the difference decides whether it is
work or a permanent absence.

**A bug or an unbuilt mechanism.** The default: the box write channel on a
shared owner (`a_box_write_on_a_shared_class_reaches_main`), the literal
`box.eval` splice (`a_box_literal_eval_is_spliced`), the dual-homed
ext-and-gem require fold (`error_highlight_library`), `Time.new`'s
class-method row (`time_new_is_a_class_method_row`), and rows of
`object_identity_and_allocation_tracing`. Each names its mechanism.

**A PERMANENT absence.** `rubyvm_iseq_serialization`'s `to_a`/`to_binary`/
`disasm` rows: zeo compiles ahead of time and has no bytecode, so a faithful
answer would mean emitting YARV it never runs. The file reads its rows apart,
because one of them (`InstructionSequence.of`) is quietly WRONG rather than
loudly absent.

A file may carry more than one kind — `rubyvm_iseq_serialization` and
`error_highlight_library` both do — which is why the headers read row by row.

## Decided divergences live in `tests/`, not here

A divergence zeo has **decided** to keep is not work, so a gap file is the
wrong home for it: the count here should mean "still to do", and an XFAIL
whose fix nobody intends never flips. Those live in the ordinary suite as
passing tests, each with a **`<name>.rb.divergence`** sidecar beside it.

The sidecar means: **`.expected` records ZEO's own output**, deliberately, and
the sidecar states why and carries ruby's answer verbatim — so the divergence
stays executable evidence rather than prose. `tools/zeo-dev bless` reads it and
records zeo instead of the oracle, which keeps these goldens machine-recorded
like every other one.

| file | why zeo answers differently |
|---|---|
| `sort_with_comparator`, `narrowed_element_local_pin` | ruby's `Array#sort` is unstable (`ruby_qsort`) and zeo's is stable, so equal comparator keys come out in a different order. Matching means porting `ruby_qsort` into a hot path to reproduce an order ruby does not promise. |
| `ractor_move_traversal_accidents`, `ractor_move_io_and_range` | CRuby guts objects as it walks, so a REFUSED move has already destroyed the source and a duplicated reference husks; zeo validates the whole graph first. Its IO handles are `Arc`-shared and its Range is an inline value, so neither can husk. |
| `a_proc_isolation_message_lists_every_outer_variable` | the variable list's order is the enclosing iseq's local table, and its membership is what CRuby's peephole left behind. |
| `kernel_scope_intrinsics_dynamic_send` | a method row cannot see its caller's block or locals, and widening `Frame` to carry them taxes every call. |
| `a_computed_require_of_a_bundled_gem`, `a_box_cannot_require_a_spliced_feature` | a computed or box-side target is opaque to the splice, so the automatic answer is "embed every gem the program can see". `--embed-sources` is the deliberate opt-in. |
| `singleton_body_class_and_self_path` | a class written in a `class << self` body goes to the enclosing module, not the singleton class, so its singleton methods reach it by bare name. |
| `an_eval_singleton_prepend_is_refused` | a snippet's compile registers nothing, so a singleton `prepend` inside `eval` is refused loudly instead of dropped silently. |
| `a_row_ruby_writes_in_ruby_names_its_source` | rows CRuby writes in `<internal:>` Ruby answer a real `#source_location`; zeo's Rust rows answer `nil` (the vendored-Ruby alternative measured 22.7x slower). |

## The XFAIL contract

Each gap runs through `crates/zeo-tests/tests/gaps.rs` (a `cargo test`/nextest target)
in **`Mode::Xfail`**:

- A gap that still **diverges** from its golden → the test **PASSES** (the
  known failure is still there — expected).
- A gap that starts **matching** ruby → the test **FAILS** with a "GAP FIXED —
  promote" message. Promote it into the zeo-authored suite (`tests/`) with:

  ```sh
  tools/zeo-dev promote-gap foo   # moves foo.rb + sidecars to tests/, verifies it
  ```

  Promote to **`tests/`**, not `tests/spinel/` — that dir mirrors the vendored
  spinel corpus and only the sync tool writes there.

  A promoted file's goldens carry its OLD path (`gaps/foo.rb:12`), and output
  that embeds the file's own relative path (an rspec backtrace, a seed-driven
  shuffle over example ids) shifts with the move — so re-bless it in its new
  home right after: `tools/zeo-dev bless foo`. If the source builds paths from
  `__dir__`, adjust them for the shallower directory first.

Both stdout **and stderr** are compared, byte-exactly, after the shared
normalization in `crates/zeo-tests/tests/support/golden.rs` (line endings, the
source path, and object addresses — `0x` + 16 hex digits → `0xADDR`, since
those are process-random on both sides).

## Goldens are recorded from ruby, never hand-written

The `.expected` is the **ruby 4.0.6 oracle** output (the target zeo must
eventually produce). Record/refresh it with:

```sh
tools/zeo-dev bless gap::   # all gaps (the filter is required by design)
tools/zeo-dev bless foo     # one gap
```

## Adding a gap

Drop in `foo.rb`, then `tools/zeo-dev bless foo` to
capture its golden. If zeo already matches ruby, the test will tell you it's not
a gap — put it in the corpus instead.

Keep at least one gap here: `datatest-stable` panics rather than reporting zero
cases, so an empty directory breaks the suite. If the last one is ever fixed,
retire `crates/zeo-tests/tests/gaps.rs` and its `[[test]]` entry along with it.
