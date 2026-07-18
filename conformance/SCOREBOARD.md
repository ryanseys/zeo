# Conformance scoreboard

Suite `spinel` — **1470/1986 passing (74.0%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `00a8a4d`

| verdict | count |
|---|---|
| PASS | 1470 |
| FAIL_OUTPUT | 447 |
| FAIL_COMPILE | 61 |
| FAIL_RUSTC | 3 |
| FAIL_RUN | 1 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 4 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **1986** |

## Top failure categories

| blocked | bucket | cluster | sample test |
|---|---|---|---|
| 176 | missing-builtin-method | P | analyze_fail/attributes_non_symbol |
| 33 | arity-panic | g | bool_nil_immediate_edges |
| 30 | spike-misc | ? | array_bsearch_find_any |
| 20 | missing-core-const | P | argf_class_no_args |
| 8 | pattern-shapes | k | array_conformance_batch5 |
| 8 | unknown-class | ? | catch_throw_ensure |
| 6 | auto-362e8305 | ? | bundle_classd_13 |
| 5 | splat | a | enum_chain_splat_sliceb_wave6 |
| 4 | range-shapes | h | bundle_tiny_num |
| 3 | auto-20dcf25f | ? | anon_struct_local |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
