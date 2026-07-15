# Gap triage

Failing tests grouped by normalized failure message, ranked by how many
tests each gap blocks. Clusters refer to the implementation plan's gap
families. Oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]`.

| cluster | bucket | blocked | sample test | sample message |
|---|---|---|---|---|
| ? | toplevel-node-in-expr | 561 | arity_keyword_argument_errors | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| P | missing-builtin-method | 225 | array_bsearch_find_any | uncaught exception: undefined method 'bsearch' for an instance of Array |
| ? | spike-misc | 114 | anon_block_forward | spinelc: an anonymous `&` block-forwarding argument (forwarding the enclosing method's own `&block` onward without naming it) isn't supported yet (spike scope) |
| ? | implicit-self-call | 65 | array_eq_cross_kind | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ! | rustc-failure | 47 | argv_gc | spinelc: rustc failed compiling the generated program (source left at /var/folders/7r/0kdzlwm12_19f3qc1j8w5vjr0000gn/T/spinelc-gen-47879-ThreadId1.rs) |
| ? | dynamic-receiver-call | 43 | alias_method_dispatch | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| P | missing-core-const | 43 | argf_class_no_args | uncaught exception: uninitialized constant ARGF |
| g | arity-panic | 24 | array_callforms_strbuf_cycle | uncaught exception: wrong number of arguments (given 1, expected 0) |
| a | param-shapes | 23 | block_param_destructure | spinelc: only plain required parameters are supported before a `*rest` (spike scope) |
| ? | unknown-class | 22 | const_path_unresolved_require | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| d | class-level-state | 18 | class_ivar_in_class_method | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| b | dyn-dispatch-kwargs | 13 | file_readlines | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| a | splat | 9 | bare_return_in_initialize | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| c | multi-assign | 6 | bundle_classd_12 | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-14bb593e | 5 | bundle_misc_c_26 | spinelc: unsupported syntax at "$1" (spike handles only what the 7 example programs need) |
| k | pattern-shapes | 5 | array_conformance_batch5 | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| g | super-arity | 5 | class_ancestors_superclass | uncaught exception: undefined method 'superclass' for class Dog |
| a | arg-forwarding | 4 | block_param_distribution | spinelc: `...` forwarding isn't supported yet (spike scope) -- needs a real Proc runtime, a later phase |
| ? | auto-1dc88be1 | 4 | ffi_poly_int_array | spinelc: cannot load such file -- json |
| a | interpolation-shapes | 4 | embedded_var | spinelc: unsupported string interpolation part (spike scope) |
| ? | auto-2b9e30a9 | 3 | const_env_defined_parity | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-56c6c16e | 3 | defined_classification | spinelc: unsupported syntax at "__FILE__" (spike handles only what the 7 example programs need) |
| ? | auto-62255d30 | 3 | enumerator_generator_each | uncaught exception: no implicit conversion of NilClass into Array |
| ? | alias-inherited | 2 | alias_attr_reader_source | spinelc: `alias title name`: `name` must already be defined earlier in the same class/module body (spike scope) -- aliasing an inherited method isn't supported yet |
| ? | auto-16db4916 | 2 | back_ref | spinelc: unsupported syntax at "$&" (spike handles only what the 7 example programs need) |
| ? | auto-693fd5cf | 2 | gsub_with_hash_replacement | uncaught exception: no implicit conversion of Hash into String |
| ? | auto-74de9804 | 2 | regex_hex_shorthand | error: unrecognized escape sequence |
| ? | auto-8e5c856a | 2 | bundle_misc_c_33 | spinelc: cannot load such file -- time |
| ? | auto-ceeffdaa | 2 | format_raise_dollarbang_symproc | uncaught exception: malformed format string - %$ |
| ? | auto-db1701f3 | 2 | bundle_sym | spinelc: unsupported syntax at "__ENCODING__" (spike handles only what the 7 example programs need) |
| ? | auto-e1c6d05b | 2 | each_slice_cons_to_a | uncaught exception: no implicit conversion of NilClass into Integer |
| ? | auto-f715b554 | 2 | i1017 | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| f | dynamic-require | 2 | for_over_hash_and_bigint_when | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| h | range-shapes | 2 | range_size_infinite_local | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-0746d056 | 1 | interp_symbol | spinelc: unsupported syntax at ":\"hello_#{x}\"" (spike handles only what the 7 example programs need) |
| ? | auto-0f1fc15b | 1 | when_splat_rational_complex | spinelc: unsupported syntax at "*a" (spike handles only what the 7 example programs need) |
| ? | auto-0f30f7e4 | 1 | post_execution | spinelc: unsupported syntax at "END {\n  puts \"last-1\"\n}" (spike handles only what the 7 example programs need) |
| ? | auto-176da354 | 1 | range_and_array_range_args | uncaught exception: invalid argument - 1..6 |
| ? | auto-1ce7f942 | 1 | string_multiply_overflow | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-20dcf25f | 1 | anon_struct_local | spinelc: `Struct.new` outside a constant assignment isn't supported (AOT: write `Name = Struct.new(:a, :b)`) |
| ? | auto-2d0f16c6 | 1 | ffi_gem_compat | spinelc: cannot load such file -- ffi |
| ? | auto-2d4c0f69 | 1 | bundle_io_sys | spinelc: unsupported syntax at "`printf \'AB\\\\000CD\\\\000EF\' > #{path}`" (spike handles only what the 7 example programs need) |
| ? | auto-2febf97c | 1 | regexp_quantifier_alt_loopback | error: look-around, including look-ahead and look-behind, is not supported |
| ? | auto-376ca277 | 1 | initialize_copy_super_object | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-3814d0b1 | 1 | float_hex_encoding_isa_bool_wave10 | uncaught exception: invalid value for Float(): "0x1p4" |
| ? | auto-3943f115 | 1 | struct_bare_super_initialize | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-41318f62 | 1 | integer_shift_float_and_coerce | uncaught exception: Float can't be coerced into Integer |
| ? | auto-4208b3f2 | 1 | splat_value_positions | spinelc: unsupported syntax at "*[1, 2]" (spike handles only what the 7 example programs need) |
| ? | auto-4f1ab327 | 1 | poly_array_index_to_h | uncaught exception: wrong element type Flat (expected array) |
| ? | auto-50835bcc | 1 | alias_under_modifier | spinelc: unsupported syntax at "alias aliased original" (spike handles only what the 7 example programs need) |
| ? | auto-5acee2ea | 1 | rindex_regexp_polypoly_merge | uncaught exception: no implicit conversion of Regexp into String |
| ? | auto-5c693233 | 1 | mutex_synchronize_block | spinelc: cannot load such file -- monitor |
| ? | auto-5c727403 | 1 | data_super_kwarg | spinelc: unsupported syntax at "x: x * 100, y: y + 1" (spike handles only what the 7 example programs need) |
| ? | auto-5d152f37 | 1 | pp_store_float_inf_literal | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-5d6a192c | 1 | string_to_i_base_zero | uncaught exception: invalid radix 0 |
| ? | auto-6203e981 | 1 | hash_shorthand_str | spinelc: unsupported syntax at "name:" (spike handles only what the 7 example programs need) |
| ? | auto-67a2b9f2 | 1 | splat_return | spinelc: unsupported syntax at "*x" (spike handles only what the 7 example programs need) |
| ? | auto-6bc687b9 | 1 | native_binding_poc | spinelc: `native_func` takes a symbol, an array of argument type constants, and a return type constant, e.g. `native_func :encode64, [String], String` |
| ? | auto-6dc7ea47 | 1 | require_first_line | spinelc: cannot load such file -- optparse |
| ? | auto-7098380b | 1 | match_tilde_pre_post | spinelc: unsupported syntax at "$`" (spike handles only what the 7 example programs need) |
| ? | auto-76817b19 | 1 | shareable_constant | spinelc: unsupported syntax at "FOO = [1, 2, 3]" (spike handles only what the 7 example programs need) |
| ? | auto-7f91b6c6 | 1 | constant_path | spinelc: cannot load such file -- stringio |
| ? | auto-836ee342 | 1 | bundle_classd_49 | spinelc: unsupported syntax at "undef foo" (spike handles only what the 7 example programs need) |
| ? | auto-844df666 | 1 | sort_mixed_type_raises | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-84f00bdc | 1 | massign_splat_rhs | spinelc: unsupported syntax at "*1" (spike handles only what the 7 example programs need) |
| ? | auto-867ee324 | 1 | super_into_included_module | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-867f036d | 1 | struct_custom_initialize_super | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-93fff1fc | 1 | alias_global | spinelc: unsupported syntax at "alias $copy $orig" (spike handles only what the 7 example programs need) |
| ? | auto-94cf42d6 | 1 | hash_shorthand | spinelc: unsupported syntax at "x:" (spike handles only what the 7 example programs need) |
| ? | auto-981fd6c7 | 1 | string_index_assign_forms | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-a2980bed | 1 | require_io_console_winsize | spinelc: cannot load such file -- io/console |
| ? | auto-a7cbe375 | 1 | module_cvars | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-a8d5a4ab | 1 | symbol_to_proc_after_positional | spinelc: define_method's second argument must be a block |
| ? | auto-abc2c8fb | 1 | kernel_array_format_negx_wave10 | uncaught exception: not a real: "2+3i" |
| ? | auto-acfd47ba | 1 | control | spinelc: unsupported syntax at "__LINE__" (spike handles only what the 7 example programs need) |
| ? | auto-af496ce5 | 1 | sort_by_array_key | uncaught exception: comparison failed |
| ? | auto-b09b7981 | 1 | pre_execution | spinelc: unsupported syntax at "BEGIN {\n  puts \"first\"\n}" (spike handles only what the 7 example programs need) |
| ? | auto-ba7c5361 | 1 | struct_inherit | spinelc: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-bbd37818 | 1 | each_cons_block_destructure | uncaught exception: no implicit conversion of String into Array |
| ? | auto-c831d0c2 | 1 | regexp_inline_comment | error: unrecognized flag |
| ? | auto-ceeff52b | 1 | printf_percent_b | uncaught exception: malformed format string - %# |
| ? | auto-cef06551 | 1 | format_hex_float | uncaught exception: malformed format string - %a |
| ? | auto-d04215d7 | 1 | bignum_downto_upto_to_a | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-d981d064 | 1 | to_h_with_block | uncaught exception: wrong element type Integer (expected array) |
| ? | auto-def07d6b | 1 | i1009 | error: backreferences are not supported |
| ? | auto-e0b8f18c | 1 | undef_method_raises | spinelc: unsupported syntax at "undef meth" (spike handles only what the 7 example programs need) |
| ? | auto-f82b092f | 1 | regex_capture_scan_toproc_count | uncaught exception: no implicit conversion of Regexp into Integer |
| a | double-splat | 1 | hash_double_splat | spinelc: double-splat (`**expr`) in a hash literal isn't supported yet (spike scope) |
| b | kwargs-call-shape | 1 | kwargs_instance_dispatch | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| j | regex-sugar | 1 | regexp_named_capture_match | spinelc: `=~`'s named-capture auto-binding sugar (synthesizing a local per named group) isn't supported yet (spike scope) -- bind the `MatchData` explicitly via `#match`/`#[]` instead |
| d | singleton-class | 1 | singleton_class_block | spinelc: unsupported syntax at "class << Greeter\n  def hello; \"hello\"; end\nend" (spike handles only what the 7 example programs need) |

List one bucket's tests: `cargo run -p xtask -- conformance triage --bucket <name>`.
