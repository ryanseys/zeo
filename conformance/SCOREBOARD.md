# Conformance scoreboard

Suite `spinel` — **1539/1986 passing (77.5%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `10bfe81`

| verdict | count |
|---|---|
| PASS | 1539 |
| FAIL_OUTPUT | 378 |
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
| 31 | spike-misc | ? | proc_capture_enclosing_lambda | a lambda escaping from inside another escaping block isn't supported yet (spike scope) |
| 16 | missing-method:ffi_func | P | ffi_ptr_nil | uncaught exception: undefined method 'ffi_func' for module LibC |
| 8 | unknown-class | ? | defined_guard_dead_branch | unknown class/module `MissingRoot::Sub` |
| 6 | auto-362e8305 | ? | bundle_hash | internal error: entered unreachable code: an own-only name is by definition not in captured_locals (see call.rs's split) |
| 5 | missing-method:ffi_lib | P | ffi_const_nested_module_path | uncaught exception: undefined method 'ffi_lib' for module Outer::CMath |
| 5 | splat | a | pattern_rightward_oneline | expected a `*name` splat in this array pattern (spike scope) |
| 4 | arity-panic | g | bundle_tiny_string | uncaught exception: wrong number of arguments (given 1, expected 0) |
| 4 | missing-method:[]= | P | bundle_misc_c_10 | uncaught exception: undefined method '[]=' for class Fiber |
| 4 | missing-method:attributes | P | compile_time_attributes | uncaught exception: undefined method 'attributes' for class CompileTimeAttributeHolder |
| 4 | missing-method:current | P | bundle_misc_a | uncaught exception: undefined method 'current' for class Fiber |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
