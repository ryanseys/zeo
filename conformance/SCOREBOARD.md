# Conformance scoreboard

Suite `spinel` — **1272/1901 passing** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `c92e34c`

| verdict | count |
|---|---|
| PASS | 1272 |
| FAIL_OUTPUT | 543 |
| FAIL_COMPILE | 82 |
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
| 279 | missing-builtin-method | P | array_chain_no_arg |
| 38 | spike-misc | ? | array_bsearch_find_any |
| 30 | arity-panic | g | block_forward_poly |
| 15 | unknown-class | ? | catch_throw_ensure |
| 13 | missing-core-const | P | argf_class_no_args |
| 7 | pattern-shapes | k | array_conformance_batch5 |
| 5 | auto-f715b554 | ? | i1017 |
| 5 | splat | a | enum_chain_splat_sliceb_wave6 |
| 4 | auto-1dc88be1 | ? | ffi_poly_int_array |
| 4 | dynamic-require | f | for_over_hash_and_bigint_when |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
