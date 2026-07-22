# Conformance scoreboard

Suite `spinel` — **2183/2295 passing (95.1%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25] [--disable-error_highlight --disable-did_you_mean]` — zeo `8ce93b2`

| verdict | count |
|---|---|
| PASS | 2183 |
| FAIL_OUTPUT | 109 |
| FAIL_COMPILE | 1 |
| FAIL_RUSTC | 0 |
| FAIL_RUN | 0 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 2 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **2295** |

## Top failure categories

| blocked | bucket | cluster | sample test | sample message |
|---|---|---|---|---|
| 5 | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-a2be | ? | issue_3193 | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/issue_3193.rb:109:in '<main>' |
| 2 | auto-users-ryanseys-dev-zeo-crates-xtask-conformance-corpus-4d1b | ? | issue_3119 | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/issue_3119.rb:8:in '<main>': no receiver is available (ArgumentError) |
| 2 | missing-method:>= | P | gc_stat_string_heap | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/gc_stat_string_heap.rb:13:in '<main>': undefined method '>=' for nil (NoMethodError) |
| 1 | arity-panic | g | str_method_nil_arg_no_segv | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/str_method_nil_arg_no_segv.rb:19:in '<main>': wrong number of arguments (given 0, expected 1+) (ArgumentError) |
| 1 | auto-error-unrecognized-escape-sequence-regexperror-2794 | ? | regexp_line_anchors | error: unrecognized escape sequence (RegexpError) |
| 1 | auto-expected-numeric-value-got-ni-4730 | ? | rational_complex_wave9 | expected a numeric value, got 2+3i |
| 1 | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-01c0 | ? | compile_time_attribute_singular | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/compile_time_attribute_singular.rb:1:in '<main>' |
| 1 | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-0827 | ? | string_split_inline_arg_gc_root | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/string_split_inline_arg_gc_root.rb:43:in '<main>' |
| 1 | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-1c7a | ? | param_lengthlike_body_widen | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/param_lengthlike_body_widen.rb:44:in '<main>' |
| 1 | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-2467 | ? | hash_each_with_object | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/hash_each_with_object.rb:15:in '<main>' |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
