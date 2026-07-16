# Gap triage

Failing tests grouped by normalized failure message, ranked by how many
tests each gap blocks. Clusters refer to the implementation plan's gap
families. Oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]`.

| cluster | bucket | blocked | sample test | sample message |
|---|---|---|---|---|
| P | missing-builtin-method | 350 | alias_method_dispatch | uncaught exception: undefined method 'vocalize' for an instance of Animal |
| ? | spike-misc | 41 | array_bsearch_find_any | uncaught exception: Array#bsearch's find-any mode (a numeric block result) isn't supported yet (spike scope) |
| g | arity-panic | 32 | array_bounds_guards | uncaught exception: wrong number of arguments (given 1, expected 0) |
| ? | unknown-class | 26 | case_in_hash_pattern_object | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| P | missing-core-const | 13 | argf_class_no_args | uncaught exception: uninitialized constant ARGF |
| ! | rustc-failure | 9 | bundle_class_14 | spinelc: rustc failed compiling the generated program (source at /var/folders/7r/0kdzlwm12_19f3qc1j8w5vjr0000gn/T/spinelc-gen-3d1c7c5bf17c2178.rs) |
| k | pattern-shapes | 7 | array_conformance_batch5 | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-1dc88be1 | 4 | ffi_poly_int_array | spinelc: cannot load such file -- json |
| ? | auto-693fd5cf | 4 | gsub_with_hash_replacement | uncaught exception: no implicit conversion of Hash into String |
| ? | auto-74de9804 | 4 | module_function_str_method_param | error: unrecognized escape sequence |
| ? | auto-f715b554 | 4 | i1017 | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| a | splat | 4 | enum_chain_splat_sliceb_wave6 | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | toplevel-node-in-expr | 4 | class_eval_reopen | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| f | dynamic-require | 3 | for_over_hash_and_bigint_when | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | alias-inherited | 2 | alias_attr_reader_source | spinelc: `alias title name`: `name` must already be defined earlier in the same class/module body (spike scope) -- aliasing an inherited method isn't supported yet |
| ? | auto-03f2cac6 | 2 | enum_terminal_chunk_zip_lazy | uncaught exception: no block given (yield) |
| ? | auto-2febf97c | 2 | regexp_captures_family | error: look-around, including look-ahead and look-behind, is not supported |
| ? | auto-3e862739 | 2 | array_cycle_bounded | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-5acee2ea | 2 | rindex_regexp_polypoly_merge | uncaught exception: no implicit conversion of Regexp into String |
| ? | auto-8e5c856a | 2 | bundle_misc_c_33 | spinelc: cannot load such file -- time |
| ? | auto-ceeffdaa | 2 | format_raise_dollarbang_symproc | uncaught exception: malformed format string - %$ |
| ? | auto-d46bf646 | 2 | proc_return_escape_localjump | uncaught signal escaped the top level: Return(99) |
| ? | auto-f82b092f | 2 | i1015 | uncaught exception: no implicit conversion of Regexp into Integer |
| h | range-shapes | 2 | range_size_infinite_local | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-02ee698a | 1 | bignum_receiver_methods | uncaught exception: Integer can't be coerced into Integer |
| ? | auto-0b8761b3 | 1 | case_in_range_pattern | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-176da354 | 1 | range_and_array_range_args | uncaught exception: invalid argument - 1..6 |
| ? | auto-1ce7f942 | 1 | string_multiply_overflow | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-20dcf25f | 1 | anon_struct_local | spinelc: `Struct.new` outside a constant assignment isn't supported (AOT: write `Name = Struct.new(:a, :b)`) |
| ? | auto-221106e8 | 1 | numeric_coerce | uncaught exception: Money can't be coerced into Integer |
| ? | auto-2d0f16c6 | 1 | ffi_gem_compat | spinelc: cannot load such file -- ffi |
| ? | auto-2d4c0f69 | 1 | bundle_io_sys | spinelc: unsupported syntax at "`printf \'AB\\\\000CD\\\\000EF\' > #{path}`" (spike handles only what the 7 example programs need) |
| ? | auto-347ede9b | 1 | object_default_display | uncaught exception:  |
| ? | auto-376ca277 | 1 | initialize_copy_super_object | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-37b0b172 | 1 | hash_sort_pairs | uncaught exception: comparison of Array with Array failed |
| ? | auto-3814d0b1 | 1 | float_hex_encoding_isa_bool_wave10 | uncaught exception: invalid value for Float(): "0x1p4" |
| ? | auto-3943f115 | 1 | struct_bare_super_initialize | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-3b2b9cc5 | 1 | open_class_object | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-3fa590af | 1 | array_splice_exceptions | uncaught exception: index -7 too small for array; minimum: -3 |
| ? | auto-41318f62 | 1 | integer_shift_float_and_coerce | uncaught exception: Float can't be coerced into Integer |
| ? | auto-434a127c | 1 | time_at_kinds_string_ctor | uncaught exception: can't convert Rational into an exact number |
| ? | auto-503269d5 | 1 | hash_range_enum_extensions | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-50835bcc | 1 | alias_under_modifier | spinelc: unsupported syntax at "alias aliased original" (spike handles only what the 7 example programs need) |
| ? | auto-52e6d3e6 | 1 | bundle_misc_c_23 | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-567065dd | 1 | comparable_clamp_parity | uncaught exception: comparison of Money with NilClass failed |
| ? | auto-5c693233 | 1 | mutex_synchronize_block | spinelc: cannot load such file -- monitor |
| ? | auto-5c727403 | 1 | data_super_kwarg | spinelc: unsupported syntax at "x: x * 100, y: y + 1" (spike handles only what the 7 example programs need) |
| ? | auto-5d152f37 | 1 | pp_store_float_inf_literal | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-5d6a192c | 1 | string_to_i_base_zero | uncaught exception: invalid radix 0 |
| ? | auto-65bd6b6e | 1 | float_round_half | uncaught exception: Hash can't be coerced into Float |
| ? | auto-6b001200 | 1 | float_quo_step_by | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-6bc687b9 | 1 | native_binding_poc | spinelc: `native_func` takes a symbol, an array of argument type constants, and a return type constant, e.g. `native_func :encode64, [String], String` |
| ? | auto-6dc7ea47 | 1 | require_first_line | spinelc: cannot load such file -- optparse |
| ? | auto-76817b19 | 1 | shareable_constant | spinelc: unsupported syntax at "FOO = [1, 2, 3]" (spike handles only what the 7 example programs need) |
| ? | auto-7c6fb6fc | 1 | bundle_array_b | uncaught exception: comparison of Symbol with Symbol failed |
| ? | auto-7d203286 | 1 | bundle_misc_c_26 | error: invalid escape sequence found in character class |
| ? | auto-7f91b6c6 | 1 | constant_path | spinelc: cannot load such file -- stringio |
| ? | auto-844df666 | 1 | sort_mixed_type_raises | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-85bd1557 | 1 | proc_keyword_param | uncaught exception: NilClass can't be coerced into Integer |
| ? | auto-867ee324 | 1 | super_into_included_module | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-867f036d | 1 | struct_custom_initialize_super | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-8945269a | 1 | kernel_rational_string_zerodenom | uncaught exception: can't convert String into Rational |
| ? | auto-90e848fa | 1 | string_with_embedded_nul | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-981fd6c7 | 1 | string_index_assign_forms | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-9b378cab | 1 | analyze_fail/instance_exec_no_block | uncaught exception: tried to create Proc object without a block (in `instance_exec') |
| ? | auto-a2980bed | 1 | require_io_console_winsize | spinelc: cannot load such file -- io/console |
| ? | auto-a7cbe375 | 1 | module_cvars | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-a8d5a4ab | 1 | symbol_to_proc_after_positional | spinelc: define_method's second argument must be a block |
| ? | auto-abc2c8fb | 1 | kernel_array_format_negx_wave10 | uncaught exception: not a real: "2+3i" |
| ? | auto-af496ce5 | 1 | sort_by_array_key | uncaught exception: comparison failed |
| ? | auto-b6bfb564 | 1 | warn_category | {category: deprecated} |
| ? | auto-ba7c5361 | 1 | struct_inherit | spinelc: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-c831d0c2 | 1 | regexp_inline_comment | error: unrecognized flag |
| ? | auto-ceefe5e0 | 1 | format_dynamic_named | uncaught exception: malformed format string - %* |
| ? | auto-ceeff52b | 1 | printf_percent_b | uncaught exception: malformed format string - %# |
| ? | auto-cef00b42 | 1 | format_named | uncaught exception: malformed format string - %< |
| ? | auto-cef029d8 | 1 | format_binary | uncaught exception: malformed format string - %B |
| ? | auto-cef06551 | 1 | format_hex_float | uncaught exception: malformed format string - %a |
| ? | auto-d02546fb | 1 | struct_yield_initialize | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-d04215d7 | 1 | bignum_downto_upto_to_a | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-d453bd61 | 1 | enumerator_size | uncaught exception: can't convert Proc into Integer |
| ? | auto-dd0fbc13 | 1 | rational_ctor_exact | uncaught exception: can't convert Float into Rational |
| ? | auto-def07d6b | 1 | i1009 | error: backreferences are not supported |
| ? | auto-dfbd22ee | 1 | float_round_truncate_ndigits | uncaught exception: NaN |
| ? | auto-ebd75c85 | 1 | stdin_io | uncaught exception: not a file |
| a | double-splat | 1 | hash_double_splat | spinelc: double-splat (`**expr`) in a hash literal isn't supported yet (spike scope) |
| a | interpolation-shapes | 1 | interp_adjacent_concat | spinelc: unsupported string interpolation part (spike scope) |
| d | singleton-class | 1 | singleton_class_block | spinelc: unsupported syntax at "class << Greeter\n  def hello; \"hello\"; end\nend" (spike handles only what the 7 example programs need) |
| g | super-arity | 1 | reopen_split_superclass_dispatch | spinelc: superclass mismatch for class Sub |

List one bucket's tests: `cargo run -p xtask -- conformance triage --bucket <name>`.
