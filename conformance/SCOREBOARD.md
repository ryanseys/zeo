# Conformance scoreboard

Suite `spinel` — **2169/2295 passing (94.5%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25] [--disable-error_highlight --disable-did_you_mean]` — zeo `8cfddef`

| verdict | count |
|---|---|
| PASS | 2169 |
| FAIL_OUTPUT | 123 |
| FAIL_COMPILE | 1 |
| FAIL_RUSTC | 0 |
| FAIL_RUN | 0 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 2 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **2295** |

## Top failure categories

| blocked | bucket | cluster | sample test | sample message |
|---|---|---|---|---|
| 5 | missing-const:OpenStruct::Warning | P | issue_3135 | uncaught exception: uninitialized constant OpenStruct::Warning (NameError) |
| 4 | arity-panic | g | bundle_tiny_string | uncaught exception: wrong number of arguments (given 2, expected 0..1) (ArgumentError) |
| 4 | missing-method:attributes | P | compile_time_attributes | uncaught exception: undefined method 'attributes' for class CompileTimeAttributeHolder (NoMethodError) |
| 2 | auto-index-too-small-for-array-minimum-indexerror-4878 | ? | array_splice_exceptions | uncaught exception: index -7 too small for array; minimum: -3 (IndexError) |
| 2 | auto-no-block-given-yield-localjumperror-1be0 | ? | string_enum_inspect_source | uncaught exception: no block given (yield) (LocalJumpError) |
| 2 | auto-no-receiver-is-available-argumenterror-234c | ? | issue_3002 | uncaught exception: no receiver is available (ArgumentError) |
| 2 | missing-method:>= | P | gc_stat_string_heap | uncaught exception: undefined method '>=' for nil (NoMethodError) |
| 2 | missing-method:define_method | P | analyze_fail/instance_exec_def_in_block | uncaught exception: undefined method 'define_method' for an instance of BoxPlus (NoMethodError) |
| 2 | missing-method:length | P | param_lengthlike_body_widen | uncaught exception: undefined method 'length' for nil (NoMethodError) |
| 1 | auto-can-coerce-complex-into-float-typeerror-9cc0 | ? | symbol_nil_bool_float_batch | uncaught exception: can't coerce Complex into Float (TypeError) |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
