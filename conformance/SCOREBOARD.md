# Conformance scoreboard

Suite `spinel` — **1492/1986 passing (75.1%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `73e9623`

| verdict | count |
|---|---|
| PASS | 1492 |
| FAIL_OUTPUT | 425 |
| FAIL_COMPILE | 61 |
| FAIL_RUSTC | 3 |
| FAIL_RUN | 1 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 4 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **1986** |

## Top failure categories

| blocked | bucket | cluster | sample test | sample message |
|---|---|---|---|---|
| 35 | arity-panic | g | string_conformance_batch3 | uncaught exception: wrong number of arguments (given 0, expected 1) |
| 31 | spike-misc | ? | minmax_by_count | Enumerable#max_by with arguments isn't supported yet (spike scope) |
| 16 | missing-method:ffi_func | P | ffi_ptr_nil | uncaught exception: undefined method 'ffi_func' for module LibC |
| 8 | pattern-shapes | k | hash_numeric_wave11 | Enumerable#any? with a pattern argument (`===` form) isn't supported yet (spike scope) |
| 8 | unknown-class | ? | defined_guard_dead_branch | unknown class/module `MissingRoot::Sub` |
| 6 | auto-362e8305 | ? | bundle_hash | internal error: entered unreachable code: an own-only name is by definition not in captured_locals (see call.rs's split) |
| 5 | missing-method:ffi_lib | P | ffi_const_nested_module_path | uncaught exception: undefined method 'ffi_lib' for module Outer::CMath |
| 5 | splat | a | pattern_rightward_oneline | expected a `*name` splat in this array pattern (spike scope) |
| 4 | missing-method:[]= | P | bundle_misc_c_10 | uncaught exception: undefined method '[]=' for class Fiber |
| 4 | missing-method:attributes | P | compile_time_attributes | uncaught exception: undefined method 'attributes' for class CompileTimeAttributeHolder |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
