# Conformance scoreboard

Suite `spinel` — **1352/1901 passing** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `d9249b4`

| verdict | count |
|---|---|
| PASS | 1352 |
| FAIL_OUTPUT | 465 |
| FAIL_COMPILE | 80 |
| FAIL_RUSTC | 0 |
| FAIL_RUN | 1 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 3 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **1901** |

## Top failure categories

| blocked | bucket | cluster | sample test |
|---|---|---|---|
| 208 | missing-builtin-method | P | array_conformance_batch |
| 40 | spike-misc | ? | array_bsearch_find_any |
| 28 | arity-panic | g | bundle_string_c |
| 15 | unknown-class | ? | catch_throw_ensure |
| 13 | missing-core-const | P | argf_class_no_args |
| 8 | pattern-shapes | k | array_conformance_batch5 |
| 5 | auto-f715b554 | ? | i1017 |
| 5 | splat | a | enum_chain_splat_sliceb_wave6 |
| 4 | auto-1dc88be1 | ? | ffi_poly_int_array |
| 4 | range-shapes | h | bundle_tiny_num |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
