# Conformance scoreboard

Suite `spinel` — **1926/2231 passing (86.3%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `5bcfa6e`

| verdict | count |
|---|---|
| PASS | 1926 |
| FAIL_OUTPUT | 275 |
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
| 3 | auto-uncaught-signal-escaped-the-top-level-break-00b3 | ? | issue_3024b | uncaught signal escaped the top level: Break(7) |
| 2 | auto-can-convert-hash-into-an-exact-number-typeerror-6187 | ? | time_strftime_z_minimal | uncaught exception: can't convert Hash into an exact number (TypeError) |
| 2 | auto-can-iterate-from-string-typeerror-987c | ? | bundle_tiny_num | uncaught exception: can't iterate from String (TypeError) |
| 2 | auto-index-too-small-for-array-minimum-indexerror-4878 | ? | array_splice_exceptions | uncaught exception: index -7 too small for array; minimum: -3 (IndexError) |
| 2 | auto-no-block-given-yield-localjumperror-1be0 | ? | string_enum_inspect_source | uncaught exception: no block given (yield) (LocalJumpError) |
| 2 | missing-const:TCPServer | P | socket_tcp_thread | uncaught exception: uninitialized constant TCPServer (NameError) |
| 2 | missing-method:>= | P | gc_stat_string_heap | uncaught exception: undefined method '>=' for nil (NoMethodError) |
| 2 | missing-method:[] | P | param_body_hash_inference | uncaught exception: undefined method '[]' for nil (NoMethodError) |

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
