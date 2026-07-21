# Conformance scoreboard

Suite `spinel` — **1995/2231 passing (89.4%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25] [--disable-error_highlight --disable-did_you_mean]` — spinel-rs `7529b9e`

| verdict | count |
|---|---|
| PASS | 1995 |
| FAIL_OUTPUT | 206 |
| FAIL_COMPILE | 0 |
| FAIL_RUSTC | 0 |
| FAIL_RUN | 0 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 2 |
| ORACLE_FAIL | 0 |
| SKIP | 28 |
| **TOTAL** | **2231** |

## Top failure categories

| blocked | bucket | cluster | sample test | sample message |
|---|---|---|---|---|
| 4 | arity-panic | g | bundle_tiny_string | uncaught exception: wrong number of arguments (given 2, expected 0..1) (ArgumentError) |
| 4 | missing-method:attributes | P | compile_time_attributes | uncaught exception: undefined method 'attributes' for class CompileTimeAttributeHolder (NoMethodError) |
| 2 | auto-index-too-small-for-array-minimum-indexerror-4878 | ? | array_splice_exceptions | uncaught exception: index -7 too small for array; minimum: -3 (IndexError) |
| 2 | auto-no-block-given-yield-localjumperror-1be0 | ? | string_enum_inspect_source | uncaught exception: no block given (yield) (LocalJumpError) |
| 2 | missing-const:TCPServer | P | socket_tcp_thread | uncaught exception: uninitialized constant TCPServer (NameError) |
| 2 | missing-method:>= | P | gc_stat_string_heap | uncaught exception: undefined method '>=' for nil (NoMethodError) |
| 2 | missing-method:[] | P | param_body_hash_inference | uncaught exception: undefined method '[]' for nil (NoMethodError) |
| 2 | missing-method:define_method | P | analyze_fail/instance_exec_def_in_block | uncaught exception: undefined method 'define_method' for an instance of BoxPlus (NoMethodError) |
| 2 | missing-method:length | P | param_lengthlike_body_widen | uncaught exception: undefined method 'length' for nil (NoMethodError) |
| 1 | auto-can-coerce-complex-into-float-typeerror-9cc0 | ? | symbol_nil_bool_float_batch | uncaught exception: can't coerce Complex into Float (TypeError) |

## Skipped tests

| test | reason |
|---|---|
| empty_array_push_ptr | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_array_specs_decl | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_binstr_recv | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_binstr_ws_frame | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_buffer_reader | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_callback | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_cflags_fold_forms | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_const | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_const_leaf_collision | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_const_nested_module_path | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_foreign_ptr_gc | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_int_arg_bigint | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_int_arg_poly_value | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_libc_libm_basic | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_poly_int_array | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_ptr_array | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_ptr_int_literal | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_ptr_nil | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_str_arg_poly_value | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_struct | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_variadic | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_void_return | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| ffi_write_roundtrip | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| i1011 | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| i1017 | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| sp_crypto_basic | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| sp_crypto_sha1 | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |
| sp_net_basic | private ffi_* DSL: CRuby raises NoMethodError for these directives (no require, not real-gem API) |

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
