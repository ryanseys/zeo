# Conformance scoreboard

Suite `spinel` — **1142/1834 passing** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `c1fbae5`

| verdict | count |
|---|---|
| PASS | 1142 |
| FAIL_OUTPUT | 589 |
| FAIL_COMPILE | 100 |
| FAIL_RUSTC | 0 |
| FAIL_RUN | 1 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 2 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **1834** |

## Top failure categories

| blocked | bucket | cluster | sample test |
|---|---|---|---|
| 335 | missing-builtin-method | P | alias_method_dispatch |
| 39 | spike-misc | ? | array_bsearch_find_any |
| 24 | unknown-class | ? | case_in_hash_pattern_object |
| 16 | arity-panic | g | block_forward_poly |
| 13 | missing-core-const | P | argf_class_no_args |
| 7 | pattern-shapes | k | array_conformance_batch5 |
| 4 | auto-1dc88be1 | ? | ffi_poly_int_array |
| 4 | auto-693fd5cf | ? | gsub_with_hash_replacement |
| 4 | auto-74de9804 | ? | module_function_str_method_param |
| 4 | auto-f715b554 | ? | i1017 |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
