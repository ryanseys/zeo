# Conformance scoreboard

Suite `spinel` — **1766/2074 passing (85.1%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `c8afc07`

| verdict | count |
|---|---|
| PASS | 1766 |
| FAIL_OUTPUT | 301 |
| FAIL_COMPILE | 7 |
| FAIL_RUSTC | 0 |
| FAIL_RUN | 0 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 0 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **2074** |

## Top failure categories

| blocked | bucket | cluster | sample test | sample message |
|---|---|---|---|---|
| 16 | missing-method:ffi_func | P | ffi_ptr_nil | uncaught exception: undefined method 'ffi_func' for module LibC |
| 5 | missing-method:ffi_lib | P | ffi_const_nested_module_path | uncaught exception: undefined method 'ffi_lib' for module Outer::CMath |
| 4 | missing-method:attributes | P | compile_time_attributes | uncaught exception: undefined method 'attributes' for class CompileTimeAttributeHolder |
| 4 | missing-method:new | P | issue_2968 | uncaught exception: undefined method 'new' for class Dir |
| 3 | arity-panic | g | str_method_nil_arg_no_segv | uncaught exception: wrong number of arguments (given 0, expected 1+) |
| 3 | auto-no-block-given-yield-cac6 | ? | string_enum_inspect_source | uncaught exception: no block given (yield) |
| 3 | missing-method:+ | P | enumerator_ops | uncaught exception: undefined method '+' for an instance of Enumerator |
| 3 | missing-method:[] | P | struct_methods | uncaught exception: undefined method '[]' for class S |
| 3 | missing-method:define_method | P | value_position_misc | uncaught exception: undefined method 'define_method' for main |
| 3 | missing-method:ffi_buffer | P | ffi_write_roundtrip | uncaught exception: undefined method 'ffi_buffer' for module Buf |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
