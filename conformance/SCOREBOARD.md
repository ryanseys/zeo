# Conformance scoreboard

Suite `spinel` — **1470/1986 passing (74.0%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `674e524`

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

| blocked | bucket | cluster | sample test | sample message |
|---|---|---|---|---|
| 33 | arity-panic | g | data_with_unknown_kw | uncaught exception: wrong number of arguments (given 2, expected 0) |
| 30 | spike-misc | ? | valued_break_proc | a lambda escaping from inside another escaping block isn't supported yet (spike scope) |
| 16 | missing-method:ffi_func | P | ffi_ptr_nil | uncaught exception: undefined method 'ffi_func' for module LibC |
| 8 | missing-const:Random | P | issue_2869 | uncaught exception: uninitialized constant Random |
| 8 | pattern-shapes | k | hash_numeric_wave11 | Enumerable#any? with a pattern argument (`===` form) isn't supported yet (spike scope) |
| 8 | unknown-class | ? | defined_guard_dead_branch | unknown class/module `MissingRoot::Sub` |
| 7 | missing-method:[] | P | hash_empty_literal_and_map | uncaught exception: undefined method '[]' for class Hash |
| 6 | auto-362e8305 | ? | bundle_hash | internal error: entered unreachable code: an own-only name is by definition not in captured_locals (see call.rs's split) |
| 5 | missing-method:ffi_lib | P | ffi_const_nested_module_path | uncaught exception: undefined method 'ffi_lib' for module Outer::CMath |
| 5 | missing-method:value | P | thread_preempt | uncaught exception: undefined method 'value' for an instance of Thread |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
