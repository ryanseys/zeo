# Conformance scoreboard

Suite `spinel` — **1812/2160 passing (83.9%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `bdaa652`

| verdict | count |
|---|---|
| PASS | 1812 |
| FAIL_OUTPUT | 342 |
| FAIL_COMPILE | 3 |
| FAIL_RUSTC | 1 |
| FAIL_RUN | 0 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 2 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **2160** |

## Top failure categories

| blocked | bucket | cluster | sample test | sample message |
|---|---|---|---|---|
| 16 | missing-method:ffi_func | P | ffi_ptr_nil | uncaught exception: undefined method 'ffi_func' for module LibC |
| 5 | arity-panic | g | issue_3093 | uncaught exception: wrong number of arguments (given 1, expected 0) |
| 5 | missing-method:ffi_lib | P | ffi_const_nested_module_path | uncaught exception: undefined method 'ffi_lib' for module Outer::CMath |
| 4 | missing-method:attributes | P | compile_time_attributes | uncaught exception: undefined method 'attributes' for class CompileTimeAttributeHolder |
| 4 | missing-method:key | P | issue_3027 | uncaught exception: undefined method 'key' for an instance of KeyError |
| 4 | missing-method:new | P | issue_2968 | uncaught exception: undefined method 'new' for class Dir |
| 3 | auto-no-block-given-yield-cac6 | ? | string_enum_inspect_source | uncaught exception: no block given (yield) |
| 3 | missing-const:Signal | P | issue_3105 | uncaught exception: uninitialized constant Signal |
| 3 | missing-method:+ | P | enumerator_ops | uncaught exception: undefined method '+' for an instance of Enumerator |
| 3 | missing-method:define_method | P | value_position_misc | uncaught exception: undefined method 'define_method' for main |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
