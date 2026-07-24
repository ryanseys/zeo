# Corpus provenance

`tests/spinel/` is vendored from the `test/` suite of
[spinel](https://github.com/matz/spinel) (MIT License, Copyright (c) 2024-
Yukihiro Matsumoto), zeo's C-emitting predecessor: ~2,300 golden-output
programs (`*.rb` + `.rb.expected` stdout snapshots, `.err.expected` /
`.args` / `.stdin` sidecars, and fixture subdirectories), every snapshot
oracle-verified against real `ruby`.

Programs run from the `tests/` working directory, so a handful of sidecar
`.args` paths and the source paths embedded in backtrace goldens name the
suite as `spinel/...` (upstream's `test/...` prefix was rewritten on the
move here). The spinel `rbs/` and `rbs-seed/` trees (RBS extraction, out of
zeo's scope) are not vendored.

This copy is zeo's living suite -- it may diverge from spinel's as tests
are added, ported (e.g. spinel's `ffi_*` intrinsic tests moving to the real
`ffi` gem API), or re-oracled.

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

## `analyze_fail/` retired

Both `analyze_fail/` cases (`dynamic_define_method`, `nonliteral_define_method_each`)
were spinel-SUBSET rejections of dynamic `define_method(name)`. zeo, targeting
full CRuby, COMPILES and runs those (empty output, matching `ruby`), so they moved
to the corpus proper as passing tests and the `analyze_fail/` harness entry was
dropped. Compile-rejection coverage lives in the e2e suite
(`compile_project(...).unwrap_err()`).

## How the corpus runs

The corpus is run by `cargo test --test spinel` (datatest-stable; see
`crates/zeo/tests/spinel.rs` + `tests/support/golden.rs`), one nextest case per
`.rb`, diffing zeo's stdout+stderr against the committed ruby-oracle
`.rb.expected`. `ZEO_BLESS=1 cargo test --test spinel` re-records the goldens.
The old bespoke `xtask conformance run`/scoreboard harness was retired.

New spinel tests are imported by `scripts/import-spinel-corpus.sh
[SPINEL_TEST_DIR]`, which TRIAGES each against zeo: a test zeo matches `ruby` on
lands here (a passing corpus case), one it diverges on lands in `../gaps/` (an
XFAIL gap). It skips tests already vendored and the `REMOVED.txt` list.
