# Conformance gaps (expected-to-fail)

Each file here is a Ruby program zeo does **not yet** run the way `ruby`
does. We check them in and track them so we can grind them down over time.

The file format is the same as the rest of the corpus: `<name>.rb` plus a
`<name>.rb.expected` stdout golden, with optional `.err.expected`, `.args`,
and `.stdin` sidecars. The `.expected` always records the **ruby 4.0.6
oracle** — the output zeo must eventually produce. Every gap's header
comment names its real cause.

## How to work with gaps

**Add a gap.** Drop in `foo.rb`, then record its golden:

```sh
`cargo xtask bless foo
```

If zeo already matches ruby, the test tells you it is not a gap — put the
file in the ordinary corpus instead.

**Refresh goldens.** Goldens are recorded from ruby, never hand-written:

```sh
`cargo xtask bless gap::   # all gaps (the filter is required by design)
`cargo xtask bless foo     # one gap
```

**Promote a fixed gap.** When zeo starts matching ruby, the gap's test fails
with a "GAP FIXED — promote" message. Move it into the zeo-authored suite:

```sh
`cargo xtask promote-gap foo   # moves foo.rb + sidecars to tests/, verifies it
```

Promote to `tests/`, not `tests/spinel/` — that directory mirrors the
vendored spinel corpus, and only the sync tool writes there. A promoted
file's goldens still carry its old path (`gaps/foo.rb:12`), and output that
embeds the file's own relative path shifts with the move — so re-bless it in
its new home right after: `cargo xtask bless foo`. If the source builds
paths from `__dir__`, adjust them for the shallower directory first.

Keep at least one gap here: `datatest-stable` panics rather than reporting
zero cases, so an empty directory breaks the suite. If the last gap is ever
fixed, retire `crates/zeo/tests/gaps.rs` and its `[[test]]` entry with it.

## The XFAIL contract

Each gap runs through `crates/zeo/tests/gaps.rs` (a nextest target) in
`Mode::Xfail`:

- The gap still **diverges** from its golden → the test **passes**. The
  known failure is still there, as expected.
- The gap starts **matching** ruby → the test **fails** with the promote
  message above.

Both stdout **and stderr** are compared byte-exactly, after the shared
normalization in `crates/zeo-tests/src/golden.rs`: line endings, the source
path, and object addresses (`0x` + 16 hex digits → `0xADDR`, because those
are process-random on both sides).

A gap may be a program that makes the compiler **panic**. The compile runs
in a child `zeo` process, so a panic records as one more stderr divergence
instead of taking the test binary down.

## Two kinds of gap

Read a gap's header to see which kind it is. The difference decides whether
the file is work or a permanent absence.

**A bug or an unbuilt mechanism.** The default. Examples: the box write
channel on a shared owner (`a_box_write_on_a_shared_class_reaches_main`),
the literal `box.eval` splice (`a_box_literal_eval_is_spliced`), the
dual-homed ext-and-gem require fold (`error_highlight_library`),
`Time.new`'s class-method row (`time_new_is_a_class_method_row`), and rows
of `object_identity_and_allocation_tracing`. Each names its mechanism.

**A permanent absence.** `rubyvm_iseq_serialization`'s
`to_a`/`to_binary`/`disasm` rows: zeo compiles ahead of time and has no
bytecode, so a faithful answer would mean emitting YARV it never runs. The
file reads its rows apart, because one of them (`InstructionSequence.of`)
is quietly wrong rather than loudly absent.

A file may carry both kinds — `rubyvm_iseq_serialization` and
`error_highlight_library` do — which is why the headers read row by row.

## Decided divergences live in `tests/`, not here

A divergence zeo has **decided** to keep is not work, so a gap file is the
wrong home for it: the count here should mean "still to do", and an XFAIL
whose fix nobody intends never flips. Decided divergences live in the
ordinary suite as passing tests, each with a `<name>.rb.divergence` sidecar.

The sidecar means: the `.expected` records **zeo's own output**,
deliberately. The sidecar states why and carries ruby's answer verbatim, so
the divergence stays executable evidence rather than prose. `cargo xtask
bless` reads the sidecar and records zeo instead of the oracle, which keeps
these goldens machine-recorded like every other one.

| file | why zeo answers differently |
|---|---|
| `sort_with_comparator`, `narrowed_element_local_pin` | ruby's `Array#sort` is unstable (`ruby_qsort`) and zeo's is stable, so equal comparator keys come out in a different order. Matching means porting `ruby_qsort` into a hot path to reproduce an order ruby does not promise. |
| `ractor_move_traversal_accidents`, `ractor_move_io_and_range` | CRuby guts objects as it walks, so a refused move has already destroyed the source and a duplicated reference husks; zeo validates the whole graph first. Its IO handles are `Arc`-shared and its Range is an inline value, so neither can husk. |
| `a_proc_isolation_message_lists_every_outer_variable` | the variable list's order is the enclosing iseq's local table, and its membership is what CRuby's peephole left behind. |
| `kernel_scope_intrinsics_dynamic_send` | a method row cannot see its caller's block or locals, and widening `Frame` to carry them taxes every call. |
| `a_computed_require_of_a_bundled_gem`, `a_box_cannot_require_a_spliced_feature` | a computed or box-side target is opaque to the splice, so the automatic answer is "embed every gem the program can see". `--embed-sources` is the deliberate opt-in. |
| `singleton_body_class_and_self_path` | a class written in a `class << self` body goes to the enclosing module, not the singleton class, so its singleton methods reach it by bare name. |
| `an_eval_singleton_prepend_is_refused` | a snippet's compile registers nothing, so a singleton `prepend` inside `eval` is refused loudly instead of dropped silently. |
| `a_row_ruby_writes_in_ruby_names_its_source` | rows CRuby writes in `<internal:>` Ruby answer a real `#source_location`; zeo's Rust rows answer `nil` (the vendored-Ruby alternative measured 22.7x slower). |
