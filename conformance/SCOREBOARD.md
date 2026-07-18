# Conformance scoreboard

Suite `spinel` — **1440/1968 passing (73.2%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `381df57`

| verdict | count |
|---|---|
| PASS | 1440 |
| FAIL_OUTPUT | 429 |
| FAIL_COMPILE | 91 |
| FAIL_RUSTC | 3 |
| FAIL_RUN | 1 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 4 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **1968** |

## Top failure categories

| blocked | bucket | cluster | sample test |
|---|---|---|---|
| 164 | missing-builtin-method | P | array_conformance_batch |
| 49 | spike-misc | ? | array_bsearch_find_any |
| 33 | arity-panic | g | bool_nil_immediate_edges |
| 19 | unknown-class | ? | catch_throw_ensure |
| 15 | missing-core-const | P | argf_class_no_args |
| 8 | pattern-shapes | k | array_conformance_batch5 |
| 6 | splat | a | enum_chain_splat_sliceb_wave6 |
| 4 | range-shapes | h | bundle_tiny_num |
| 4 | toplevel-node-in-expr | ? | class_eval_reopen |
| 3 | auto-20dcf25f | ? | anon_struct_local |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
