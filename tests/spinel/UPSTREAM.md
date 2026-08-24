# Corpus provenance

`tests/spinel/` is vendored from the `test/` suite of
[spinel](https://github.com/matz/spinel) (MIT License, Copyright (c) 2024-
Yukihiro Matsumoto), zeo's C-emitting predecessor. Upstream's license text is
reproduced verbatim beside this file, in [`LICENSE`](LICENSE), as the MIT
license requires of any redistribution: ~3,000 golden-output
programs (`*.rb` + `.rb.expected` stdout snapshots, `.err.expected` /
`.args` / `.stdin` sidecars, and fixture subdirectories), every snapshot
oracle-verified against real `ruby`.

Last synced from upstream `master` at **`c55d9bdb`** (2026-08-17). Record the
sha on every sync: without it, drift is invisible -- the sync that added this
line found 444 upstream tests missing and 79 vendored bodies stale.

Programs run from the `tests/` working directory, so a handful of sidecar
`.args` paths and the source paths embedded in backtrace goldens name the
suite as `spinel/...` (upstream's `test/...` prefix was rewritten on the
move here). The spinel `rbs/` and `rbs-seed/` trees (RBS extraction, out of
zeo's scope) are not vendored.

This copy is zeo's living suite -- it may diverge from spinel's as tests
are added, ported (e.g. spinel's `ffi_*` intrinsic tests moving to the real
`ffi` gem API), or re-oracled.

## The divergence that shapes the corpus: frozen string literals

Spinel freezes string literals **by default, with no opt-out** -- a
`# frozen_string_literal: false` pragma warns and is ignored, and
`--disable=frozen-string-literal` is rejected (spinel `docs/limitations.md`).
CRuby 4.0.6 does not: `"abc".frozen?` is `false`.

This is the single biggest structural difference between the two corpora, and
it is why upstream tests increasingly write `+"str"` where a CRuby-native test
would write `"str"`. Two consequences:

- Newly imported tests keep whatever the upstream author wrote. `+"str"` is
  valid CRuby and its ruby-oracled golden is true, so it imports as-is -- but
  it exercises a fresh unfrozen string rather than CRuby's mutable literal.
  Do not mistake a `+` for a bug, and do not strip it.
- A vendored test must NOT be refreshed from upstream just because the body
  changed. Of the 79 bodies that differed at the `c55d9bdb` sync, 38 differed
  only by this rewrite; adopting them would have traded mutable-literal
  coverage for nothing. See `PORTED.txt`.

## Removed tests

Tests exercising spinel-only **compile-time** DSL macros with no CRuby
analog were removed: both real `ruby` and zeo raise `NoMethodError`
identically, so they only ever pinned spinel compiler intrinsics, not
language behavior. Removed: `compile_time_attribute_singular`,
`compile_time_attribute_wrapped_record`, `compile_time_attributes`,
`compile_time_define_method_predicates` (spinel's `attribute`/`attributes`
class macro), and `native_binding_poc` (spinel's `native_obj`/`native_func`
native-binding DSL).

Also removed: `regexp_inline_options` -- a spinel-era test whose own comments
declare it a set of "deliberate divergences from CRuby". Its `.expected` was
never re-oracled; matching real `ruby` would require emitting Onigmo's internal
regex-compiler warnings (`regular expression has redundant nested repeat
operator`, printed twice with a source-line prefix) which the vendored
Oniguruma binding fundamentally cannot produce, plus Onigmo-specific conditional
validation. The regex ENGINE semantics it also touched (inline `(?m:)` DOTALL,
the absence operator `(?~...)`, line anchors) are covered by `Engine::Onig` and
the `regexp_onig` example/e2e.

Removed at the `c55d9bdb` sync, for the same reason -- spinel-native FFI DSL
with no `ffi`-gem analog, so a port would assert nothing: `ffi_buffer_bounds_ok`
(a COMPILE-TIME bounds refusal on `ffi_buffer` accessors; the gem has no
compile-time buffer declaration), `ffi_const_case_when` (spinel's private
`ffi_const` value table read against the constant table), and `ffi_source`
(embeds a C fragment into the generated translation unit).

## Ported tests

A test whose upstream body pins spinel behaviour, but whose SUBJECT has a
CRuby-valid spelling, is re-authored here rather than removed. `PORTED.txt`
lists every such stem so `--refresh` never overwrites the port. Ported at the
`c55d9bdb` sync:

- `ffi_read_write_widths` -- `ffi_buffer` + `ffi_read_u32`/`ffi_write_i16`
  class macros become `FFI::MemoryPointer#put_*`/`#get_*` at an offset.
- `ffi_type_list_forms` -- `ffi_func` becomes `attach_function`, which takes
  the same argument-type array, so the forms under test (a frozen constant,
  `[:float] * 2`, a literal) carry over unchanged.
- `socket_constants` -- `Socket::TCP_KEEPIDLE`/`TCP_KEEPINTVL`/`TCP_KEEPCNT`
  are Linux-only (CRuby on macOS raises `NameError`), so the golden would be
  machine-specific. Replaced with the portable `TCP_NODELAY`.
- `proc_arg_channel_released` -- spinel's `GC.stat["bytes"]` becomes
  `GC.stat[:heap_live_slots]`. CRuby's `GC.stat` is Symbol-keyed and has no
  "bytes" statistic, so the String subscript is `nil` and the comparison
  raises `NoMethodError`.

`ruby_engine_name` is vendored UNPORTED: it asserts `RUBY_ENGINE == "spinel"`,
and the CRuby golden (`"ruby"`, `true`, `false`) is exactly what zeo answers.

## `analyze_fail/` retired

Both `analyze_fail/` cases (`dynamic_define_method`, `nonliteral_define_method_each`)
were spinel-SUBSET rejections of dynamic `define_method(name)`. zeo, targeting
full CRuby, COMPILES and runs those (empty output, matching `ruby`), so they moved
to the corpus proper as passing tests and the `analyze_fail/` harness entry was
dropped. Compile-rejection coverage lives in the e2e suite
(`compile_project(...).unwrap_err()`).

## How the corpus runs

The corpus is run by `cargo nextest run -p zeo-tests --test spinel`
(datatest-stable; see `crates/zeo-tests/tests/spinel.rs` +
`crates/zeo-tests/tests/support/golden.rs`), one nextest case per `.rb`,
diffing zeo's stdout+stderr against the committed ruby-oracle `.rb.expected`.
`tools/zeo-dev bless spinel::` re-records the goldens from ruby -- and
is the ONLY thing that does: `golden.rs` honours `ZEO_BLESS_FROM_TOOL`, which
only `zeo-dev bless` sets, so a bare `ZEO_BLESS=1 cargo test` does nothing. The
old bespoke `xtask conformance run`/scoreboard harness was retired.

## Syncing from upstream

The importer that produced this corpus has been deleted; the sync below is
historical.
closes both drift axes, blessing every golden from the ruby 4.0.6 oracle --
never from spinel's own `.expected`, which is oracled against ruby 3.4 and
carries spinel's deviations.

- default (**import**): vendors tests not present yet.
- `--refresh`: re-vendors an already-vendored test whose UPSTREAM BODY has
  changed. Skips every stem in `PORTED.txt`.
- Both modes skip `REMOVED.txt`, and both TRIAGE afterwards: a test zeo matches
  `ruby` on stays here, one it diverges on moves to `../gaps/` as an XFAIL gap.
- `--no-triage` copies and blesses without running the suite, so a sync that
  does both modes pays for one corpus run instead of two.

A full sync is:

```sh
cd ~/dev/spinel && git fetch upstream && git status   # confirm the sha
cd ~/dev/zeo
cargo nextest run -p zeo-tests --test spinel --no-fail-fast
# every FAIL moves to ../gaps/ with a header naming its cause
```

Then update the sha at the top of this file.
