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
divergence does** stays here, with the reason in its header, rather than
becoming a passing test that would pin behaviour ruby does not promise.

## Three kinds of file live here

Read a gap's header for which one it is; the difference decides whether it is
work or a decision.

**A bug or an unbuilt mechanism.** The default, and most of the directory:
run-time boxes (`a_box_*`, `a_class_written_in_a_box_escapes_it`,
`a_top_level_def_in_a_box_escapes_it`), the cycle collector and heap
enumeration (`a_reference_cycle_is_never_reclaimed`,
`there_is_no_heap_enumeration`), the singleton chain's missing linearization
(`a_doubled_module_in_a_singleton_chain`, `remove_method_on_a_mixed_in_module`),
reflection surfaces
(`builtin_method_parameter_names`, `a_require_gated_builtin_row_is_always_present`,
`a_struct_backed_builtin_class_is_not_a_struct`,
`the_ast_translator_answers_unknown_for_some_shapes`), and the two prism-id
files. Each names its mechanism and, where one exists, the phase that owns it.

**A DECISION, kept here rather than in a passing test.** Reproducing these
would make zeo's own behaviour worse or cost more than the divergence does:
`sort_with_comparator` and `narrowed_element_local_pin` (ruby's `Array#sort` is
unstable — `ruby_qsort` — and zeo's is stable, so equal comparator keys come
out in a different order; matching means porting `ruby_qsort` into a hot path
for behaviour ruby's docs leave unspecified), `ractor_move_traversal_accidents`
and `ractor_move_io_and_range` (CRuby guts objects as it walks, so a REFUSED
move has already destroyed the source; zeo validates the whole graph first),
`a_proc_isolation_message_lists_every_outer_variable` (the variable list's
order), `kernel_scope_intrinsics_dynamic_send` (a method row cannot see its
caller's block, and widening `Frame` to carry one costs every call), and
`a_later_def_on_a_builtin_reaches_back` (a builtin's first definition is a
native row with no `Scope` to order against — revisit if the corelib gives
builtins real scopes).

**A PERMANENT absence.** `rubyvm_iseq_serialization`'s `to_a`/`to_binary`/
`disasm` rows: zeo compiles ahead of time and has no bytecode, so a faithful
answer would mean emitting YARV it never runs. The file reads its rows apart,
because one of them (`InstructionSequence.of`) is quietly WRONG rather than
loudly absent.

A file may carry more than one kind — `rubyvm_iseq_serialization` and
`error_highlight_library` both do — which is why the headers read row by row.

## The XFAIL contract

Each gap runs through `crates/zeo-tests/tests/gaps.rs` (a `cargo test`/nextest target)
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

  A promoted file's goldens carry its OLD path (`gaps/foo.rb:12`), and output
  that embeds the file's own relative path (an rspec backtrace, a seed-driven
  shuffle over example ids) shifts with the move — so re-bless it in its new
  home right after: `cargo xtask bless foo`. If the source builds paths from
  `__dir__`, adjust them for the shallower directory first.

Both stdout **and stderr** are compared, byte-exactly, after the shared
normalization in `crates/zeo-tests/tests/support/golden.rs` (line endings, the
source path, and object addresses — `0x` + 16 hex digits → `0xADDR`, since
those are process-random on both sides).

## Goldens are recorded from ruby, never hand-written

The `.expected` is the **ruby 4.0.6 oracle** output (the target zeo must
eventually produce). Record/refresh it with:

```sh
cargo xtask bless gap::   # all gaps (the filter is required by design)
cargo xtask bless foo     # one gap
```

## Adding a gap

Drop in `foo.rb`, then `cargo xtask bless foo` to
capture its golden. If zeo already matches ruby, the test will tell you it's not
a gap — put it in the corpus instead. New gaps usually arrive via
`scripts/import-spinel-corpus.sh` (spinel triage).

Keep at least one gap here: `datatest-stable` panics rather than reporting zero
cases, so an empty directory breaks the suite. If the last one is ever fixed,
retire `crates/zeo-tests/tests/gaps.rs` and its `[[test]]` entry along with it.
