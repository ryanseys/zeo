# Gap triage

Failing tests grouped by normalized failure message, ranked by how many
tests each gap blocks. Clusters refer to the implementation plan's gap
families. Oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]`.

| cluster | bucket | blocked | sample tests | sample message |
|---|---|---|---|---|
| ? | spike-misc | 30 | valued_break_proc, external_singleton_def, public_send_visibility | a lambda escaping from inside another escaping block isn't supported yet (spike scope) |
| g | arity-panic | 23 | data_inspect_in_container, data_with_unknown_kw, rational_div_family | uncaught exception: wrong number of arguments (given 1, expected 0) |
| P | missing-method:ffi_func | 16 | ffi_ptr_nil, sp_crypto_sha1, ffi_binstr_ws_frame | uncaught exception: undefined method 'ffi_func' for module LibC |
| k | pattern-shapes | 8 | hash_numeric_wave11, array_conformance_batch5, bundle_tiny_array | Enumerable#any? with a pattern argument (`===` form) isn't supported yet (spike scope) |
| ? | unknown-class | 8 | defined_guard_dead_branch, harness_batch_2453_2456, catch_throw_ensure | unknown class/module `MissingRoot::Sub` |
| ? | auto-362e8305 | 6 | bundle_hash, fiber_nested_block, proc_nonlocal_return | internal error: entered unreachable code: an own-only name is by definition not in captured_locals (see call.rs's split) |
| P | missing-method:ffi_lib | 5 | ffi_const_nested_module_path, ffi_foreign_ptr_gc, ffi_int_arg_bigint | uncaught exception: undefined method 'ffi_lib' for module Outer::CMath |
| a | splat | 5 | pattern_rightward_oneline, proc_call_kw_and_lead_splat, proc_call_splat | expected a `*name` splat in this array pattern (spike scope) |
| P | missing-method:[]= | 4 | bundle_misc_c_10, fiber_storage_nil, bundle_misc_c_08 | uncaught exception: undefined method '[]=' for class Fiber |
| P | missing-method:attributes | 4 | compile_time_attributes, compile_time_define_method_predicates, analyze_fail/attributes_non_symbol | uncaught exception: undefined method 'attributes' for class CompileTimeAttributeHolder |
| P | missing-method:current | 4 | bundle_misc_a, fiber_class_reflection, fiber_transfer_root | uncaught exception: undefined method 'current' for class Fiber |
| P | missing-method:new | 4 | proc_compose_curry_forward, basicobject_new, constant_path | uncaught exception: undefined method 'new' for class Proc |
| h | range-shapes | 4 | range_sum_endless_wave10, bundle_tiny_num, range_float_begin_iterate | Range#size on a non-Integer/beginless/endless range isn't supported (spike scope) |
| ? | auto-20dcf25f | 3 | data_define_inline_receiver, anon_struct_local, data_define_duplicate_member | `Struct.new` outside a constant assignment isn't supported (AOT: write `Name = Struct.new(:a, :b)`) |
| ? | auto-3e862739 | 3 | error_protocol_edges, poly_array_readers, array_cycle_bounded | attempt to take negative size (ArgumentError; spike scope: raised as a panic) |
| P | missing-method:+ | 3 | enumerator_ops, instance_exec_dynamic_ivar, enumerable_chain_enumerator | uncaught exception: undefined method '+' for an instance of Enumerator |
| P | missing-method:[] | 3 | struct_methods, multi_write_call_rhs_as_expr, param_body_hash_inference | uncaught exception: undefined method '[]' for class S |
| P | missing-method:define_method | 3 | value_position_misc, analyze_fail/instance_exec_def_in_block, analyze_fail/instance_exec_define_method | uncaught exception: undefined method 'define_method' for an instance of Object |
| P | missing-method:ffi_buffer | 3 | ffi_write_roundtrip, i1017, ffi_buffer_reader | uncaught exception: undefined method 'ffi_buffer' for module Buf |
| P | missing-method:length | 3 | bundle_misc_c_36, param_lengthlike_body_widen, regexp_match_data | uncaught exception: undefined method 'length' for an instance of NilClass |
| P | missing-method:report_on_exception= | 3 | thread_raise_main, thread_basic, thread_kill_raise | uncaught exception: undefined method 'report_on_exception=' for class Thread |
| ! | rustc-failure | 3 | proc_block_param_call, exception_object_surface, new_forwarded_block_stored | rustc failed compiling the generated program (source at /var/folders/7r/0kdzlwm12_19f3qc1j8w5vjr0000gn/T/spinelc-gen-14cc3095c83dd703.rs) |
| ? | auto-03f2cac6 | 2 | string_enum_inspect_source, enum_terminal_chunk_zip_lazy | uncaught exception: no block given (yield) |
| ? | auto-176da354 | 2 | rand_prng_stream, range_and_array_range_args | uncaught exception: invalid argument - 1..1000 |
| ? | auto-1b4f9693 | 2 | symbol_range_enum, range_float_type | uncaught exception: can't iterate from the given Range |
| ? | auto-3fa590af | 2 | array_splice_exceptions, bundle_array_a | uncaught exception: index -7 too small for array; minimum: -3 |
| ? | auto-434a127c | 2 | time_at_kinds_string_ctor, time_plus_rational | uncaught exception: can't convert Rational into an exact number |
| ? | auto-8e5c856a | 2 | bundle_misc_c_33, bundle_misc_c_34 | cannot load such file -- time |
| ? | auto-d46bf646 | 2 | proc_return_rescue_modifier_escape, proc_return_escape_localjump | uncaught signal escaped the top level: Return(99) |
| f | dynamic-require | 2 | require_parent, user_enumerable_each_and_for | /Users/ryanseys/dev/spinel/test/require_parent/views/articles/index.rb: cannot load such file -- /Users/ryanseys/dev/spinel/test/require_parent/views/articles/...rb |
| P | missing-const:ARGF | 2 | argf_reads_args, argf_class_no_args | uncaught exception: uninitialized constant ARGF |
| P | missing-const:Marshal | 2 | poly_hash_inspect, marshal_symlink_float | uncaught exception: uninitialized constant Marshal |
| P | missing-method:<< | 2 | thread_queue, hash_each_with_object | uncaught exception: undefined method '<<' for an instance of Queue |
| P | missing-method:>= | 2 | gc_stat_string_heap, i1021 | uncaught exception: undefined method '>=' for an instance of NilClass |
| P | missing-method:__callee__ | 2 | object_method_gaps_callee_clone_itself, method_at_toplevel | uncaught exception: undefined method '__callee__' for an instance of Object |
| P | missing-method:allocate | 2 | class_allocate, class_allocate_builtin_var | uncaught exception: undefined method 'allocate' for class Thing |
| P | missing-method:binding | 2 | binding_lvget, unsupported_feature_diagnostics | uncaught exception: undefined method 'binding' for an instance of Object |
| P | missing-method:class_eval | 2 | class_eval_reopen, class_eval_value_form | uncaught exception: undefined method 'class_eval' for class Gadget |
| P | missing-method:deconstruct_keys | 2 | issue_2866, time_deconstruct_keys | uncaught exception: undefined method 'deconstruct_keys' for an instance of Time |
| P | missing-method:dig | 2 | struct_enumerable_wave8, struct_dig | uncaught exception: undefined method 'dig' for an instance of Ou142 |
| P | missing-method:foreach | 2 | wave_followups_2833, dir_full_surface | uncaught exception: undefined method 'foreach' for class File |
| P | missing-method:name | 2 | exception_value_flow_match, thread_name | uncaught exception: undefined method 'name' for an instance of NoMethodError |
| P | missing-method:owner | 2 | method_owner_receiver, module_instance_method_unbound | uncaught exception: undefined method 'owner' for an instance of Method |
| P | missing-method:pass | 2 | thread_condvar, thread_pass_fairness | uncaught exception: undefined method 'pass' for class Thread |
| P | missing-method:private_method_defined? | 2 | method_visibility_inherit, method_visibility_attr | uncaught exception: undefined method 'private_method_defined?' for class Sub |
| P | missing-method:read | 2 | poly_keyed_hash_pipeline, io_class_methods_surface | uncaught exception: undefined method 'read' for class 'Class' |
| P | missing-method:source_location | 2 | proc_source_location, proc_source_location_var | uncaught exception: undefined method 'source_location' for an instance of Proc |
| ? | auto-1e00ef9d | 1 | super_missing_hash_slot | `super`: no `as_json` found above Rec |
| ? | auto-27bada46 | 1 | warn_category | {category: :deprecated} |
| ? | auto-2d0f16c6 | 1 | ffi_gem_compat | cannot load such file -- ffi |
| ? | auto-2d4c0f69 | 1 | bundle_io_sys | unsupported syntax at "`printf \'AB\\\\000CD\\\\000EF\' > #{path}`" (spike handles only what the 7 example programs need) |
| ? | auto-3814d0b1 | 1 | float_hex_encoding_isa_bool_wave10 | uncaught exception: invalid value for Float(): "0x1p4" |
| ? | auto-3afe88d5 | 1 | cmethod_super | `super` outside a method |
| ? | auto-454832aa | 1 | pack_float_directives | uncaught exception: no implicit conversion of Float into Integer |
| ? | auto-5c693233 | 1 | mutex_synchronize_block | cannot load such file -- monitor |
| ? | auto-5d152f37 | 1 | pp_store_float_inf_literal | assertion failed: f.is_finite() |
| ? | auto-5d6a192c | 1 | string_to_i_base_zero | uncaught exception: invalid radix 0 |
| ? | auto-63954a45 | 1 | symbol_nil_bool_float_batch | uncaught exception: can't coerce Complex into Float |
| ? | auto-6dc7ea47 | 1 | require_first_line | cannot load such file -- optparse |
| ? | auto-74de9804 | 1 | regexp_line_anchors | error: unrecognized escape sequence |
| ? | auto-767cf864 | 1 | range_bsearch_float | uncaught exception: can't do binary search for the given Range |
| ? | auto-867ee324 | 1 | super_into_included_module | `super`: no `orphan` found above E |
| ? | auto-8945269a | 1 | kernel_rational_string_zerodenom | uncaught exception: can't convert String into Rational |
| ? | auto-8c9acd6e | 1 | i1009 | uncaught exception: Parsing error at position 5: Invalid back reference |
| ? | auto-98d36e6a | 1 | time_fractional_seconds | uncaught exception: no implicit conversion of Rational into Integer |
| ? | auto-9b378cab | 1 | analyze_fail/instance_exec_no_block | uncaught exception: tried to create Proc object without a block (in `instance_exec') |
| ? | auto-a2980bed | 1 | require_io_console_winsize | cannot load such file -- io/console |
| ? | auto-a8d5a4ab | 1 | symbol_to_proc_after_positional | define_method's second argument must be a block |
| ? | auto-b07debb8 | 1 | proc_return_catch_no_leak | uncaught signal escaped the top level: Throw(tag, 5) |
| ? | auto-ba7c5361 | 1 | struct_inherit | expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-c7d3b59c | 1 | kernel_array_format_negx_wave10 | uncaught exception: can't convert String into Complex |
| ? | auto-cd5e05c6 | 1 | hash_dig | expected an Integer, got b |
| ? | auto-ceefc00e | 1 | random_methods | uncaught exception: invalid argument - 5...5 |
| ? | auto-d04215d7 | 1 | bignum_downto_upto_to_a | Integer#downto beyond i64 isn't supported (unrunnable iteration count) |
| ? | auto-d453bd61 | 1 | enumerator_size | uncaught exception: can't convert Proc into Integer |
| ? | auto-d7c58806 | 1 | numeric_edges_wave10 | uncaught exception: Hash can't be coerced into Integer |
| ? | auto-dfbd22ee | 1 | float_round_truncate_ndigits | uncaught exception: NaN |
| ? | auto-e43187a8 | 1 | string_scan_literal | uncaught exception: wrong argument type String (expected Regexp) |
| ? | auto-e6bba903 | 1 | frozen_string_literal_per_file_rev | uncaught exception: can't modify frozen String: "plain" |
| ? | auto-ebd75c85 | 1 | stdin_io | uncaught exception: not a file |
| ? | auto-ee43c0d6 | 1 | exception_value_parity | `raise`/`fail` with an explicit `cause:` keyword override isn't supported yet -- automatic cause chaining from an active `rescue` works (Exception#cause); only the explicit override is deferred |
| ? | auto-f715b554 | 1 | yield_no_block_raises | no block given (LocalJumpError) |
| ? | auto-f876aad1 | 1 | string_multiply_overflow | memory allocation of 1152921504606846976 bytes failed |
| a | interpolation-shapes | 1 | interp_adjacent_concat | unsupported string interpolation part (spike scope) |
| P | missing-const:File::ALT_SEPARATOR | 1 | file_path_predicates | uncaught exception: uninitialized constant File::ALT_SEPARATOR |
| P | missing-const:File::SEPARATOR | 1 | bundle_tiny_misc | uncaught exception: uninitialized constant File::SEPARATOR |
| P | missing-const:Interrupt | 1 | exception_reflection_surface | uncaught exception: uninitialized constant Interrupt |
| P | missing-const:Object::Integer | 1 | is_a_toplevel_scoped_const | uncaught exception: uninitialized constant Object::Integer |
| P | missing-const:Object::Kernel | 1 | sleep_return_value | uncaught exception: uninitialized constant Object::Kernel |
| P | missing-const:Object::Marshal | 1 | marshal_roundtrip | uncaught exception: uninitialized constant Object::Marshal |
| P | missing-const:Process::Status | 1 | scoped_builtin_class_values | uncaught exception: uninitialized constant Process::Status |
| P | missing-const:RUBY_DESCRIPTION | 1 | symbol_id2name_ruby_desc_minmax | uncaught exception: uninitialized constant RUBY_DESCRIPTION |
| P | missing-const:RequestDispatch::ViewHelpers | 1 | include_chain_module_nested_const | uncaught exception: uninitialized constant RequestDispatch::ViewHelpers |
| P | missing-const:SizedQueue | 1 | thread_sized_queue | uncaught exception: uninitialized constant SizedQueue |
| P | missing-method:<= | 1 | string_split_inline_arg_gc_root | uncaught exception: undefined method '<=' for an instance of NilClass |
| P | missing-method:> | 1 | string_plus_heap_gc | uncaught exception: undefined method '>' for an instance of NilClass |
| P | missing-method:>> | 1 | proc_compose_method_operand | uncaught exception: undefined method '>>' for an instance of Method |
| P | missing-method:Integer | 1 | expr_retry_equal_curry_methodobj | uncaught exception: undefined method 'Integer' for class 'Object' |
| P | missing-method:acosh | 1 | math_domain_error | uncaught exception: undefined method 'acosh' for module Math |
| P | missing-method:asctime | 1 | time_asctime | uncaught exception: undefined method 'asctime' for an instance of Time |
| P | missing-method:attribute | 1 | compile_time_attribute_singular | uncaught exception: undefined method 'attribute' for class CompileTimeSingleAttribute |
| P | missing-method:block_given? | 1 | block_given_block_param | uncaught exception: undefined method 'block_given?' for an instance of Object |
| P | missing-method:byteoffset | 1 | matchdata_values_at_byteoffset | uncaught exception: undefined method 'byteoffset' for an instance of MatchData |
| P | missing-method:caller | 1 | kernel_caller_wired | uncaught exception: undefined method 'caller' for an instance of Object |
| P | missing-method:caller_locations | 1 | caller_locations | uncaught exception: undefined method 'caller_locations' for an instance of Object |
| P | missing-method:class_exec | 1 | class_exec_def_body | uncaught exception: undefined method 'class_exec' for class C |
| P | missing-method:class_variables | 1 | module_class_var_reflection_set | uncaught exception: undefined method 'class_variables' for class C |
| P | missing-method:const_set | 1 | module_const_set | uncaught exception: undefined method 'const_set' for class Box |
| P | missing-method:count | 1 | env_full_surface | uncaught exception: undefined method 'count' for an instance of Object |
| P | missing-method:detailed_message | 1 | exception_detailed_message | uncaught exception: undefined method 'detailed_message' for an instance of RuntimeError |
| P | missing-method:divide | 1 | set_divide | uncaught exception: undefined method 'divide' for an instance of Set |
| P | missing-method:each_value | 1 | env_grep_lazy_followups | uncaught exception: undefined method 'each_value' for an instance of Object |
| P | missing-method:feed | 1 | enumerator_feed_result | uncaught exception: undefined method 'feed' for an instance of Enumerator |
| P | missing-method:ffi_callback | 1 | ffi_callback | uncaught exception: undefined method 'ffi_callback' for module L |
| P | missing-method:ffi_cflags | 1 | i1011 | uncaught exception: undefined method 'ffi_cflags' for module Pathy |
| P | missing-method:ffi_const | 1 | ffi_const | uncaught exception: undefined method 'ffi_const' for module Flags |
| P | missing-method:ffi_struct | 1 | ffi_struct | uncaught exception: undefined method 'ffi_struct' for module M |
| P | missing-method:flatten! | 1 | set_conformance_batch2 | uncaught exception: undefined method 'flatten!' for an instance of Set |
| P | missing-method:foo | 1 | poly_keyed_hash_method_dedup | uncaught exception: undefined method 'foo' for class 'Class' |
| P | missing-method:format | 1 | splat_print_builtins | uncaught exception: undefined method 'format' for an instance of Object |
| P | missing-method:frexp | 1 | float_rational_math_family | uncaught exception: undefined method 'frexp' for module Math |
| P | missing-method:ftype | 1 | file_surface_extended | uncaught exception: undefined method 'ftype' for class File |
| P | missing-method:gamma | 1 | numeric_tower_methods | uncaught exception: undefined method 'gamma' for module Math |
| P | missing-method:hello | 1 | toplevel_include_module_function | uncaught exception: undefined method 'hello' for an instance of Object |
| P | missing-method:hi | 1 | send_literal_and_user | uncaught exception: undefined method 'hi' for an instance of Mailer |
| P | missing-method:include? | 1 | param_include_body_widen | uncaught exception: undefined method 'include?' for an instance of NilClass |
| P | missing-method:included_modules | 1 | module_constants_included_modules | uncaught exception: undefined method 'included_modules' for class Dog |
| P | missing-method:kill | 1 | fiber_kill | uncaught exception: undefined method 'kill' for an instance of Fiber |
| P | missing-method:ldexp | 1 | math_poly_arg_unbox | uncaught exception: undefined method 'ldexp' for module Math |
| P | missing-method:lgamma | 1 | numeric_tower_remainder | uncaught exception: undefined method 'lgamma' for module Math |
| P | missing-method:linear_time? | 1 | regexp_encoding_introspection | uncaught exception: undefined method 'linear_time?' for class Regexp |
| P | missing-method:list | 1 | thread_list | uncaught exception: undefined method 'list' for class Thread |
| P | missing-method:names | 1 | matchdata_named_groups | uncaught exception: undefined method 'names' for an instance of MatchData |
| P | missing-method:native_obj | 1 | native_binding_poc | uncaught exception: undefined method 'native_obj' for module NB |
| P | missing-method:offset | 1 | i974 | uncaught exception: undefined method 'offset' for an instance of MatchData |
| P | missing-method:pipe | 1 | io_pipe | uncaught exception: undefined method 'pipe' for class IO |
| P | missing-method:produce | 1 | enumerator_produce | uncaught exception: undefined method 'produce' for class Enumerator |
| P | missing-method:public_method | 1 | public_method | uncaught exception: undefined method 'public_method' for an instance of String |
| P | missing-method:public_method_defined? | 1 | method_visibility | uncaught exception: undefined method 'public_method_defined?' for class Account |
| P | missing-method:putc | 1 | kernel_conversions | uncaught exception: undefined method 'putc' for an instance of Object |
| P | missing-method:raise | 1 | fiber_raise | uncaught exception: undefined method 'raise' for an instance of Fiber |
| P | missing-method:regexp | 1 | matchdata_methods | uncaught exception: undefined method 'regexp' for an instance of MatchData |
| P | missing-method:round | 1 | time_round_floor_ceil | uncaught exception: undefined method 'round' for an instance of Time |
| P | missing-method:size | 1 | lazy_size | uncaught exception: undefined method 'size' for an instance of Enumerator::Lazy |
| P | missing-method:subtract | 1 | set_full_api | uncaught exception: undefined method 'subtract' for an instance of Set |
| P | missing-method:synchronize | 1 | mutex_synchronize_ensure | uncaught exception: undefined method 'synchronize' for an instance of Mutex |
| P | missing-method:system | 1 | system_argument_list | uncaught exception: undefined method 'system' for an instance of Object |
| P | missing-method:to_a | 1 | time_to_a_to_r | uncaught exception: undefined method 'to_a' for an instance of Time |
| P | missing-method:transfer | 1 | fiber_error_guards | uncaught exception: undefined method 'transfer' for an instance of Fiber |
| P | missing-method:trap | 1 | signal_trap_stub | uncaught exception: undefined method 'trap' for an instance of Object |
| P | missing-method:try_convert | 1 | regexp_reflection | uncaught exception: undefined method 'try_convert' for class Regexp |
| P | missing-method:try_lock | 1 | thread_mutex | uncaught exception: undefined method 'try_lock' for an instance of Mutex |
| P | missing-method:union | 1 | regexp_union_strings | uncaught exception: undefined method 'union' for class Regexp |
| P | missing-method:value? | 1 | env_mutation_surface | uncaught exception: undefined method 'value?' for an instance of Object |
| P | missing-method:values | 1 | struct_to_a_values | uncaught exception: undefined method 'values' for an instance of Person |
| P | missing-method:values_at | 1 | struct_valuesat_hash_set_init_wave10 | uncaught exception: undefined method 'values_at' for an instance of Pt |
| P | missing-method:wordy | 1 | const_aliased_class_reopen_include | uncaught exception: undefined method 'wordy' for an instance of Integer |
| P | missing-method:xmlschema | 1 | time_xmlschema | uncaught exception: undefined method 'xmlschema' for an instance of Time |
| g | super-arity | 1 | reopen_split_superclass_dispatch | superclass mismatch for class Sub |

List one bucket's tests: `cargo run -p xtask -- conformance triage --bucket <name>`.
