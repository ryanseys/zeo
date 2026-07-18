# Gap triage

Failing tests grouped by normalized failure message, ranked by how many
tests each gap blocks. Clusters refer to the implementation plan's gap
families. Oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]`.

| cluster | bucket | blocked | sample test | sample message |
|---|---|---|---|---|
| P | missing-builtin-method | 164 | array_conformance_batch | uncaught exception: undefined method 'try_convert' for class Array |
| ? | spike-misc | 49 | array_bsearch_find_any | uncaught exception: Array#bsearch's find-any mode (a numeric block result) isn't supported yet (spike scope) |
| g | arity-panic | 33 | bool_nil_immediate_edges | uncaught exception: wrong number of arguments (given 1, expected 0) |
| ? | unknown-class | 19 | catch_throw_ensure | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| P | missing-core-const | 15 | argf_class_no_args | uncaught exception: uninitialized constant ARGF |
| k | pattern-shapes | 8 | array_conformance_batch5 | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| a | splat | 6 | enum_chain_splat_sliceb_wave6 | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| h | range-shapes | 4 | bundle_tiny_num | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | toplevel-node-in-expr | 4 | class_eval_reopen | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-20dcf25f | 3 | anon_struct_local | spinelc: `Struct.new` outside a constant assignment isn't supported (AOT: write `Name = Struct.new(:a, :b)`) |
| ? | auto-3d0c6827 | 3 | array_fill_block_form | uncaught exception: no implicit conversion of Range into Integer |
| ? | auto-3e862739 | 3 | array_cycle_bounded | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ! | rustc-failure | 3 | exception_object_surface | spinelc: rustc failed compiling the generated program (source at /var/folders/7r/0kdzlwm12_19f3qc1j8w5vjr0000gn/T/spinelc-gen-b94f1f62f17ab2db.rs) |
| ? | auto-03f2cac6 | 2 | enum_terminal_chunk_zip_lazy | uncaught exception: no block given (yield) |
| ? | auto-1b4f9693 | 2 | range_float_type | uncaught exception: can't iterate from the given Range |
| ? | auto-3fa590af | 2 | array_splice_exceptions | uncaught exception: index -7 too small for array; minimum: -3 |
| ? | auto-434a127c | 2 | time_at_kinds_string_ctor | uncaught exception: can't convert Rational into an exact number |
| ? | auto-5acee2ea | 2 | matchdata_value_varargs_symstr | uncaught exception: no implicit conversion of Regexp into String |
| ? | auto-8e5c856a | 2 | bundle_misc_c_33 | spinelc: cannot load such file -- time |
| ? | auto-d46bf646 | 2 | proc_return_escape_localjump | uncaught signal escaped the top level: Return(99) |
| f | dynamic-require | 2 | require_parent | spinelc: /Users/ryanseys/dev/spinel/test/require_parent/views/articles/index.rb: cannot load such file -- /Users/ryanseys/dev/spinel/test/require_parent/views/articles/...rb |
| ? | auto-02ee698a | 1 | bignum_receiver_methods | uncaught exception: Integer can't be coerced into Integer |
| ? | auto-176da354 | 1 | range_and_array_range_args | uncaught exception: invalid argument - 1..6 |
| ? | auto-1ce7f942 | 1 | string_multiply_overflow | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-1e00ef9d | 1 | super_missing_hash_slot | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-27bada46 | 1 | warn_category | {category: :deprecated} |
| ? | auto-2d0f16c6 | 1 | ffi_gem_compat | spinelc: cannot load such file -- ffi |
| ? | auto-2d4c0f69 | 1 | bundle_io_sys | spinelc: unsupported syntax at "`printf \'AB\\\\000CD\\\\000EF\' > #{path}`" (spike handles only what the 7 example programs need) |
| ? | auto-3814d0b1 | 1 | float_hex_encoding_isa_bool_wave10 | uncaught exception: invalid value for Float(): "0x1p4" |
| ? | auto-3afe88d5 | 1 | cmethod_super | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-41318f62 | 1 | integer_shift_float_and_coerce | uncaught exception: Float can't be coerced into Integer |
| ? | auto-454832aa | 1 | pack_float_directives | uncaught exception: no implicit conversion of Float into Integer |
| ? | auto-5c693233 | 1 | mutex_synchronize_block | spinelc: cannot load such file -- monitor |
| ? | auto-5d152f37 | 1 | pp_store_float_inf_literal | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-5d6a192c | 1 | string_to_i_base_zero | uncaught exception: invalid radix 0 |
| ? | auto-65bd6b6e | 1 | float_round_half | uncaught exception: Hash can't be coerced into Float |
| ? | auto-693fd5cf | 1 | string_sub_hash_replacement | uncaught exception: no implicit conversion of Hash into String |
| ? | auto-6b001200 | 1 | float_quo_step_by | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-6dc7ea47 | 1 | require_first_line | spinelc: cannot load such file -- optparse |
| ? | auto-74de9804 | 1 | regexp_line_anchors | error: unrecognized escape sequence |
| ? | auto-767cf864 | 1 | range_bsearch_float | uncaught exception: can't do binary search for the given Range |
| ? | auto-7a68b003 | 1 | time_new_in_kwarg | uncaught exception: no implicit conversion of Hash into Integer (utc_offset) |
| ? | auto-7f56d934 | 1 | integer_rational_complex_ops | uncaught exception: Rational can't be coerced into Integer |
| ? | auto-867ee324 | 1 | super_into_included_module | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-8945269a | 1 | kernel_rational_string_zerodenom | uncaught exception: can't convert String into Rational |
| ? | auto-8c9acd6e | 1 | i1009 | uncaught exception: Parsing error at position 5: Invalid back reference |
| ? | auto-98d36e6a | 1 | time_fractional_seconds | uncaught exception: no implicit conversion of Rational into Integer |
| ? | auto-9b378cab | 1 | analyze_fail/instance_exec_no_block | uncaught exception: tried to create Proc object without a block (in `instance_exec') |
| ? | auto-a2980bed | 1 | require_io_console_winsize | spinelc: cannot load such file -- io/console |
| ? | auto-a8d5a4ab | 1 | symbol_to_proc_after_positional | spinelc: define_method's second argument must be a block |
| ? | auto-aa024b53 | 1 | time_numeric_string_args | uncaught exception: no implicit conversion of String into Integer |
| ? | auto-ae9c325e | 1 | file_path_predicates | uncaught exception: no implicit conversion of Array into String (in `join') |
| ? | auto-ba7c5361 | 1 | struct_inherit | spinelc: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-c7d3b59c | 1 | kernel_array_format_negx_wave10 | uncaught exception: can't convert String into Complex |
| ? | auto-cd5e05c6 | 1 | hash_dig | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-d04215d7 | 1 | bignum_downto_upto_to_a | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| ? | auto-d453bd61 | 1 | enumerator_size | uncaught exception: can't convert Proc into Integer |
| ? | auto-d7c58806 | 1 | numeric_edges_wave10 | uncaught exception: Hash can't be coerced into Integer |
| ? | auto-dfbd22ee | 1 | float_round_truncate_ndigits | uncaught exception: NaN |
| ? | auto-e43187a8 | 1 | string_scan_literal | uncaught exception: wrong argument type String (expected Regexp) |
| ? | auto-e6bba903 | 1 | frozen_string_literal_per_file_rev | uncaught exception: can't modify frozen String: "plain" |
| ? | auto-ebd75c85 | 1 | stdin_io | uncaught exception: not a file |
| ? | auto-f715b554 | 1 | yield_no_block_raises | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| a | interpolation-shapes | 1 | interp_adjacent_concat | spinelc: unsupported string interpolation part (spike scope) |
| d | singleton-class | 1 | singleton_class_block | spinelc: unsupported syntax at "class << Greeter\n  def hello; \"hello\"; end\nend" (spike handles only what the 7 example programs need) |
| g | super-arity | 1 | reopen_split_superclass_dispatch | spinelc: superclass mismatch for class Sub |

List one bucket's tests: `cargo run -p xtask -- conformance triage --bucket <name>`.
