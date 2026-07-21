# Conformance scoreboard

Suite `spinel` — **2135/2295 passing (93.0%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25] [--disable-error_highlight --disable-did_you_mean]` — zeo `3a2e7ef`

| verdict | count |
|---|---|
| PASS | 2135 |
| FAIL_OUTPUT | 154 |
| FAIL_COMPILE | 5 |
| FAIL_RUSTC | 0 |
| FAIL_RUN | 0 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 1 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **2295** |

## Top failure categories

| blocked | bucket | cluster | sample test | sample message |
|---|---|---|---|---|
| 16 | missing-method:ffi_func | P | ffi_ptr_nil | uncaught exception: undefined method 'ffi_func' for module LibC (NoMethodError) |
| 5 | auto-cannot-load-such-file-ostruct-0485 | ? | issue_3193 | cannot load such file -- ostruct |
| 5 | missing-method:ffi_lib | P | ffi_const_nested_module_path | uncaught exception: undefined method 'ffi_lib' for module Outer::CMath (NoMethodError) |
| 4 | arity-panic | g | bundle_tiny_string | uncaught exception: wrong number of arguments (given 2, expected 0..1) (ArgumentError) |
| 4 | missing-method:attributes | P | compile_time_attributes | uncaught exception: undefined method 'attributes' for class CompileTimeAttributeHolder (NoMethodError) |
| 3 | missing-method:ffi_buffer | P | ffi_write_roundtrip | uncaught exception: undefined method 'ffi_buffer' for module Buf (NoMethodError) |
| 2 | auto-index-too-small-for-array-minimum-indexerror-4878 | ? | array_splice_exceptions | uncaught exception: index -7 too small for array; minimum: -3 (IndexError) |
| 2 | auto-no-block-given-yield-localjumperror-1be0 | ? | string_enum_inspect_source | uncaught exception: no block given (yield) (LocalJumpError) |
| 2 | missing-method:>= | P | gc_stat_string_heap | uncaught exception: undefined method '>=' for nil (NoMethodError) |
| 2 | missing-method:define_method | P | analyze_fail/instance_exec_def_in_block | uncaught exception: undefined method 'define_method' for an instance of BoxPlus (NoMethodError) |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
