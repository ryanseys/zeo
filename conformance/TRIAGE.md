# Gap triage

Failing tests grouped by normalized failure message, ranked by how many
tests each gap blocks. Clusters refer to the implementation plan's gap
families. Oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]`.

| cluster | bucket | blocked | sample test | sample message |
|---|---|---|---|---|
| P | missing-builtin-method | 279 | array_chain_no_arg | uncaught exception: undefined method 'chain' for an instance of Array |
| ? | spike-misc | 38 | array_bsearch_find_any | uncaught exception: Array#bsearch's find-any mode (a numeric block result) isn't supported yet (spike scope) |
| g | arity-panic | 30 | block_forward_poly | uncaught exception: wrong number of arguments (given 2, expected 1) |
| ? | unknown-class | 15 | catch_throw_ensure | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| P | missing-core-const | 13 | argf_class_no_args | uncaught exception: uninitialized constant ARGF |
| k | pattern-shapes | 7 | array_conformance_batch5 | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-f715b554 | 5 | i1017 | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| a | splat | 5 | enum_chain_splat_sliceb_wave6 | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-1dc88be1 | 4 | ffi_poly_int_array | spinelc: cannot load such file -- json |
| f | dynamic-require | 4 | for_over_hash_and_bigint_when | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-20dcf25f | 3 | anon_struct_local | spinelc: `Struct.new` outside a constant assignment isn't supported (AOT: write `Name = Struct.new(:a, :b)`) |
| ? | auto-3e862739 | 3 | array_cycle_bounded | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-74de9804 | 3 | regex_hex_shorthand | error: unrecognized escape sequence |
| h | range-shapes | 3 | range_float_begin_iterate | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-03f2cac6 | 2 | enum_terminal_chunk_zip_lazy | uncaught exception: no block given (yield) |
| ? | auto-1b4f9693 | 2 | range_float_type | uncaught exception: can't iterate from the given Range |
| ? | auto-2febf97c | 2 | regexp_captures_family | error: look-around, including look-ahead and look-behind, is not supported |
| ? | auto-3d0c6827 | 2 | array_fill_block_form | uncaught exception: no implicit conversion of Range into Integer |
| ? | auto-3fa590af | 2 | array_splice_exceptions | uncaught exception: index -7 too small for array; minimum: -3 |
| ? | auto-434a127c | 2 | time_at_kinds_string_ctor | uncaught exception: can't convert Rational into an exact number |
| ? | auto-8e5c856a | 2 | bundle_misc_c_33 | spinelc: cannot load such file -- time |
| ? | auto-90e848fa | 2 | string_append_binary_safe | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-d46bf646 | 2 | proc_return_escape_localjump | uncaught signal escaped the top level: Return(99) |
| ? | toplevel-node-in-expr | 2 | class_eval_reopen | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-02ee698a | 1 | bignum_receiver_methods | uncaught exception: Integer can't be coerced into Integer |
| ? | auto-176da354 | 1 | range_and_array_range_args | uncaught exception: invalid argument - 1..6 |
| ? | auto-1ce7f942 | 1 | string_multiply_overflow | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-2d0f16c6 | 1 | ffi_gem_compat | spinelc: cannot load such file -- ffi |
| ? | auto-2d4c0f69 | 1 | bundle_io_sys | spinelc: unsupported syntax at "`printf \'AB\\\\000CD\\\\000EF\' > #{path}`" (spike handles only what the 7 example programs need) |
| ? | auto-3814d0b1 | 1 | float_hex_encoding_isa_bool_wave10 | uncaught exception: invalid value for Float(): "0x1p4" |
| ? | auto-41318f62 | 1 | integer_shift_float_and_coerce | uncaught exception: Float can't be coerced into Integer |
| ? | auto-4394232b | 1 | clamp_nil_open_bounds | uncaught exception: comparison of Integer with NilClass failed |
| ? | auto-567065dd | 1 | comparable_clamp_parity | uncaught exception: comparison of Money with NilClass failed |
| ? | auto-5acee2ea | 1 | string_search_slice | uncaught exception: no implicit conversion of Regexp into String |
| ? | auto-5c693233 | 1 | mutex_synchronize_block | spinelc: cannot load such file -- monitor |
| ? | auto-5d152f37 | 1 | pp_store_float_inf_literal | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-5d6a192c | 1 | string_to_i_base_zero | uncaught exception: invalid radix 0 |
| ? | auto-65bd6b6e | 1 | float_round_half | uncaught exception: Hash can't be coerced into Float |
| ? | auto-693fd5cf | 1 | string_sub_hash_replacement | uncaught exception: no implicit conversion of Hash into String |
| ? | auto-6b001200 | 1 | float_quo_step_by | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-6bc687b9 | 1 | native_binding_poc | spinelc: `native_func` takes a symbol, an array of argument type constants, and a return type constant, e.g. `native_func :encode64, [String], String` |
| ? | auto-6dc7ea47 | 1 | require_first_line | spinelc: cannot load such file -- optparse |
| ? | auto-76817b19 | 1 | shareable_constant | spinelc: unsupported syntax at "FOO = [1, 2, 3]" (spike handles only what the 7 example programs need) |
| ? | auto-7af30852 | 1 | comparable_cmp_result_validation | uncaught exception: comparison of FDiff with FDiff failed |
| ? | auto-7d203286 | 1 | bundle_misc_c_26 | error: invalid escape sequence found in character class |
| ? | auto-7f91b6c6 | 1 | constant_path | spinelc: cannot load such file -- stringio |
| ? | auto-844df666 | 1 | sort_mixed_type_raises | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-85bd1557 | 1 | proc_keyword_param | uncaught exception: NilClass can't be coerced into Integer |
| ? | auto-867ee324 | 1 | super_into_included_module | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-8945269a | 1 | kernel_rational_string_zerodenom | uncaught exception: can't convert String into Rational |
| ? | auto-981fd6c7 | 1 | string_index_assign_forms | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-98d36e6a | 1 | time_fractional_seconds | uncaught exception: no implicit conversion of Rational into Integer |
| ? | auto-9b378cab | 1 | analyze_fail/instance_exec_no_block | uncaught exception: tried to create Proc object without a block (in `instance_exec') |
| ? | auto-a2980bed | 1 | require_io_console_winsize | spinelc: cannot load such file -- io/console |
| ? | auto-a8d5a4ab | 1 | symbol_to_proc_after_positional | spinelc: define_method's second argument must be a block |
| ? | auto-aa024b53 | 1 | time_numeric_string_args | uncaught exception: no implicit conversion of String into Integer |
| ? | auto-abc2c8fb | 1 | kernel_array_format_negx_wave10 | uncaught exception: not a real: "2+3i" |
| ? | auto-b6bfb564 | 1 | warn_category | {category: deprecated} |
| ? | auto-ba7c5361 | 1 | struct_inherit | spinelc: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-c831d0c2 | 1 | regexp_inline_comment | error: unrecognized flag |
| ? | auto-d04215d7 | 1 | bignum_downto_upto_to_a | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-d453bd61 | 1 | enumerator_size | uncaught exception: can't convert Proc into Integer |
| ? | auto-d56c139e | 1 | integer_rational_complex_ops | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-dd0fbc13 | 1 | rational_ctor_exact | uncaught exception: can't convert Float into Rational |
| ? | auto-def07d6b | 1 | i1009 | error: backreferences are not supported |
| ? | auto-dfbd22ee | 1 | float_round_truncate_ndigits | uncaught exception: NaN |
| ? | auto-e43187a8 | 1 | string_scan_literal | uncaught exception: wrong argument type String (expected Regexp) |
| ? | auto-e62722d3 | 1 | pack_float_directives | uncaught exception: unsupported pack directive: G |
| ? | auto-e62733d1 | 1 | pack_base64 | uncaught exception: unsupported pack directive: M |
| ? | auto-e6274b9b | 1 | pack_endian_modifiers | uncaught exception: unsupported pack directive: _ |
| ? | auto-ebd75c85 | 1 | stdin_io | uncaught exception: not a file |
| a | interpolation-shapes | 1 | interp_adjacent_concat | spinelc: unsupported string interpolation part (spike scope) |
| d | singleton-class | 1 | singleton_class_block | spinelc: unsupported syntax at "class << Greeter\n  def hello; \"hello\"; end\nend" (spike handles only what the 7 example programs need) |
| g | super-arity | 1 | reopen_split_superclass_dispatch | spinelc: superclass mismatch for class Sub |

List one bucket's tests: `cargo run -p xtask -- conformance triage --bucket <name>`.
