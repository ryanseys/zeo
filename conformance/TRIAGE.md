# Gap triage

Failing tests grouped by normalized failure message, ranked by how many
tests each gap blocks. Clusters refer to the implementation plan's gap
families. Oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]`.

| cluster | bucket | blocked | sample tests | sample message |
|---|---|---|---|---|
| P | missing-method:ffi_func | 16 | ffi_ptr_nil, sp_crypto_sha1, ffi_binstr_ws_frame | uncaught exception: undefined method 'ffi_func' for module LibC |
| ? | spike-misc | 16 | back_ref, external_singleton_def, require_in_conditional | the `$+` back-reference global isn't supported yet (spike scope) |
| P | missing-method:ffi_lib | 5 | ffi_const_nested_module_path, ffi_foreign_ptr_gc, ffi_int_arg_bigint | uncaught exception: undefined method 'ffi_lib' for module Outer::CMath |
| P | missing-method:attributes | 4 | compile_time_attributes, compile_time_define_method_predicates, analyze_fail/attributes_non_symbol | uncaught exception: undefined method 'attributes' for class CompileTimeAttributeHolder |
| P | missing-method:new | 4 | issue_2968, basicobject_new, dir_handle_objects | uncaught exception: undefined method 'new' for class Dir |
| g | arity-panic | 3 | str_method_nil_arg_no_segv, string_enum_arg_forms, bundle_tiny_string | uncaught exception: wrong number of arguments (given 0, expected 1+) |
| ? | auto-outside-constant-assignment-isn-supported-aot-write-f25f | 3 | data_define_duplicate_member, data_define_inline_receiver, anon_struct_local | `Struct.new` outside a constant assignment isn't supported (AOT: write `Name = Struct.new(:a, :b)`) |
| f | dynamic-require | 3 | require_parent, user_enumerable_each_and_for, issue_2978 | /Users/ryanseys/dev/spinel/test/require_parent/views/articles/index.rb: cannot load such file -- /Users/ryanseys/dev/spinel/test/require_parent/views/articles/...rb |
| P | missing-method:+ | 3 | enumerator_ops, instance_exec_dynamic_ivar, enumerable_chain_enumerator | uncaught exception: undefined method '+' for an instance of Enumerator |
| P | missing-method:[] | 3 | struct_methods, multi_write_call_rhs_as_expr, param_body_hash_inference | uncaught exception: undefined method '[]' for class S |
| P | missing-method:define_method | 3 | value_position_misc, analyze_fail/instance_exec_def_in_block, analyze_fail/instance_exec_define_method | uncaught exception: undefined method 'define_method' for main |
| P | missing-method:ffi_buffer | 3 | ffi_write_roundtrip, i1017, ffi_buffer_reader | uncaught exception: undefined method 'ffi_buffer' for module Buf |
| P | missing-method:length | 3 | bundle_misc_c_36, param_lengthlike_body_widen, regexp_match_data | uncaught exception: undefined method 'length' for nil |
| ? | auto-can-convert-hash-into-an-exact-number-778a | 2 | time_strftime_z_minimal, time_at_in_offset | uncaught exception: can't convert Hash into an exact number |
| ? | auto-cannot-load-such-file-time-856a | 2 | bundle_misc_c_34, bundle_misc_c_33 | cannot load such file -- time |
| ? | auto-index-too-small-for-array-minimum-90af | 2 | array_splice_exceptions, bundle_array_a | uncaught exception: index -7 too small for array; minimum: -3 |
| ? | auto-no-block-given-yield-cac6 | 2 | enum_terminal_chunk_zip_lazy, string_enum_inspect_source | uncaught exception: no block given (yield) |
| P | missing-const:ARGF | 2 | argf_reads_args, argf_class_no_args | uncaught exception: uninitialized constant ARGF |
| P | missing-method:>= | 2 | gc_stat_string_heap, i1021 | uncaught exception: undefined method '>=' for nil |
| P | missing-method:binding | 2 | binding_lvget, unsupported_feature_diagnostics | uncaught exception: undefined method 'binding' for main |
| P | missing-method:class_eval | 2 | class_eval_reopen, class_eval_value_form | uncaught exception: undefined method 'class_eval' for class Gadget |
| P | missing-method:read | 2 | poly_keyed_hash_pipeline, io_class_methods_surface | uncaught exception: undefined method 'read' for class 'Class' |
| P | missing-method:transfer | 2 | fiber_error_guards, fiber_transfer_root | uncaught exception: undefined method 'transfer' for an instance of Fiber |
| ? | auto-assertion-failed-is-finite-2f37 | 1 | pp_store_float_inf_literal | assertion failed: f.is_finite() |
| ? | auto-can-coerce-complex-into-float-4a45 | 1 | symbol_nil_bool_float_batch | uncaught exception: can't coerce Complex into Float |
| ? | auto-can-convert-proc-into-integer-bd61 | 1 | enumerator_size | uncaught exception: can't convert Proc into Integer |
| ? | auto-can-convert-time-into-an-exact-number-7699 | 1 | time_at_kinds_string_ctor | uncaught exception: can't convert Time into an exact number |
| ? | auto-can-iterate-from-string-c4d1 | 1 | bundle_tiny_num | uncaught exception: can't iterate from String |
| ? | auto-can-modify-frozen-string-plain-a903 | 1 | frozen_string_literal_per_file_rev | uncaught exception: can't modify frozen String: "plain" |
| ? | auto-cannot-load-such-file-ffi-16c6 | 1 | ffi_gem_compat | cannot load such file -- ffi |
| ? | auto-cannot-load-such-file-io-console-0bed | 1 | require_io_console_winsize | cannot load such file -- io/console |
| ? | auto-cannot-load-such-file-monitor-3233 | 1 | mutex_synchronize_block | cannot load such file -- monitor |
| ? | auto-cannot-load-such-file-optparse-ea47 | 1 | require_first_line | cannot load such file -- optparse |
| ? | auto-define-method-second-argument-must-be-block-a4ab | 1 | symbol_to_proc_after_positional | define_method's second argument must be a block |
| ? | auto-error-unrecognized-escape-sequence-9804 | 1 | regexp_line_anchors | error: unrecognized escape sequence |
| ? | auto-expected-an-integer-got-05c6 | 1 | hash_dig | expected an Integer, got b |
| ? | auto-expected-constant-name-or-path-or-5361 | 1 | struct_inherit | expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-expected-numeric-value-got-ni-4730 | 1 | rational_complex_wave9 | expected a numeric value, got 2+3i |
| ? | auto-hash-can-be-coerced-into-integer-8806 | 1 | numeric_edges_wave10 | uncaught exception: Hash can't be coerced into Integer |
| ? | auto-memory-allocation-of-bytes-failed-aad1 | 1 | string_multiply_overflow | memory allocation of 1152921504606846976 bytes failed |
| ? | auto-nan-22ee | 1 | float_round_truncate_ndigits | uncaught exception: NaN |
| ? | auto-no-block-given-localjumperror-b554 | 1 | yield_no_block_raises | no block given (LocalJumpError) |
| ? | auto-no-implicit-conversion-of-float-into-integer-32aa | 1 | pack_float_directives | uncaught exception: no implicit conversion of Float into Integer |
| ? | auto-no-implicit-conversion-of-rational-into-integer-6e6a | 1 | time_fractional_seconds | uncaught exception: no implicit conversion of Rational into Integer |
| ? | auto-not-file-5c85 | 1 | stdin_io | uncaught exception: not a file |
| ? | auto-outside-method-88d5 | 1 | cmethod_super | `super` outside a method |
| ? | auto-parsing-error-at-position-invalid-back-reference-cd6e | 1 | i1009 | uncaught exception: Parsing error at position 5: Invalid back reference |
| ? | auto-parsing-error-at-position-invalid-character-class-e1f7 | 1 | regexp_encoding_introspection | uncaught exception: Parsing error at position 5: Invalid character class |
| ? | auto-the-synthesized-value-class-template-for-failed-to-9692 | 1 | issue_2975 | internal error: the synthesized value-class template for `S` failed to parse |
| ? | auto-tried-to-create-proc-object-without-block-ab8d | 1 | constant_path | uncaught exception: tried to create Proc object without a block |
| ? | auto-tried-to-create-proc-object-without-block-in-8cab | 1 | analyze_fail/instance_exec_no_block | uncaught exception: tried to create Proc object without a block (in `instance_exec') |
| ? | auto-uncaught-signal-escaped-the-top-level-break-00b3 | 1 | valued_break_proc | uncaught signal escaped the top level: Break(1) |
| ? | auto-undefined-group-name-reference-nope-123c | 1 | matchdata_named_groups | undefined group name reference: nope |
| ? | auto-unsupported-syntax-at-spike-handles-only-what-the-0f69 | 1 | bundle_io_sys | unsupported syntax at "`printf \'AB\\\\000CD\\\\000EF\' > #{path}`" (spike handles only what the 7 example programs need) |
| ? | auto-with-an-explicit-keyword-override-isn-supported-yet-c0d6 | 1 | exception_value_parity | `raise`/`fail` with an explicit `cause:` keyword override isn't supported yet -- automatic cause chaining from an active `rescue` works (Exception#cause); only the explicit override is deferred |
| ? | auto-wrong-argument-type-string-expected-regexp-87a8 | 1 | string_scan_literal | uncaught exception: wrong argument type String (expected Regexp) |
| a | interpolation-shapes | 1 | interp_adjacent_concat | unsupported string interpolation part (spike scope) |
| P | missing-const:File::FNM_DOTMATCH | 1 | dir_full_surface | uncaught exception: uninitialized constant File::FNM_DOTMATCH |
| P | missing-const:OpenSSL | 1 | harness_batch_2453_2456 | uncaught exception: uninitialized constant OpenSSL |
| P | missing-const:Process::Status | 1 | scoped_builtin_class_values | uncaught exception: uninitialized constant Process::Status |
| P | missing-const:RequestDispatch::ViewHelpers | 1 | include_chain_module_nested_const | uncaught exception: uninitialized constant RequestDispatch::ViewHelpers |
| P | missing-const:Signal | 1 | signal_module_surface | uncaught exception: uninitialized constant Signal |
| P | missing-method:<< | 1 | hash_each_with_object | uncaught exception: undefined method '<<' for nil |
| P | missing-method:<= | 1 | string_split_inline_arg_gc_root | uncaught exception: undefined method '<=' for nil |
| P | missing-method:> | 1 | string_plus_heap_gc | uncaught exception: undefined method '>' for nil |
| P | missing-method:Integer | 1 | expr_retry_equal_curry_methodobj | uncaught exception: undefined method 'Integer' for class 'Object' |
| P | missing-method:[]= | 1 | bundle_misc_c_09 | uncaught exception: undefined method '[]=' for an instance of Fiber |
| P | missing-method:attr | 1 | issue_2952 | uncaught exception: undefined method 'attr' for class C001 |
| P | missing-method:attribute | 1 | compile_time_attribute_singular | uncaught exception: undefined method 'attribute' for class CompileTimeSingleAttribute |
| P | missing-method:begin | 1 | i974 | uncaught exception: undefined method 'begin' for an instance of MatchData |
| P | missing-method:block_given? | 1 | block_given_block_param | uncaught exception: undefined method 'block_given?' for main |
| P | missing-method:bytebegin | 1 | matchdata_values_at_byteoffset | uncaught exception: undefined method 'bytebegin' for an instance of MatchData |
| P | missing-method:call | 1 | proc_block_param_call | uncaught exception: undefined method 'call' for nil |
| P | missing-method:caller | 1 | kernel_caller_wired | uncaught exception: undefined method 'caller' for main |
| P | missing-method:caller_locations | 1 | caller_locations | uncaught exception: undefined method 'caller_locations' for main |
| P | missing-method:class_exec | 1 | class_exec_def_body | uncaught exception: undefined method 'class_exec' for class C |
| P | missing-method:const_set | 1 | module_const_set | uncaught exception: undefined method 'const_set' for class Box |
| P | missing-method:count | 1 | env_full_surface | uncaught exception: undefined method 'count' for an instance of Object |
| P | missing-method:detailed_message | 1 | exception_detailed_message | uncaught exception: undefined method 'detailed_message' for an instance of RuntimeError |
| P | missing-method:each_value | 1 | env_grep_lazy_followups | uncaught exception: undefined method 'each_value' for an instance of Object |
| P | missing-method:ffi_callback | 1 | ffi_callback | uncaught exception: undefined method 'ffi_callback' for module L |
| P | missing-method:ffi_cflags | 1 | i1011 | uncaught exception: undefined method 'ffi_cflags' for module Pathy |
| P | missing-method:ffi_const | 1 | ffi_const | uncaught exception: undefined method 'ffi_const' for module Flags |
| P | missing-method:ffi_struct | 1 | ffi_struct | uncaught exception: undefined method 'ffi_struct' for module M |
| P | missing-method:foo | 1 | poly_keyed_hash_method_dedup | uncaught exception: undefined method 'foo' for class 'Class' |
| P | missing-method:format | 1 | splat_print_builtins | uncaught exception: undefined method 'format' for main |
| P | missing-method:hello | 1 | toplevel_include_module_function | uncaught exception: undefined method 'hello' for main |
| P | missing-method:hi | 1 | send_literal_and_user | uncaught exception: undefined method 'hi' for an instance of Mailer |
| P | missing-method:include? | 1 | param_include_body_widen | uncaught exception: undefined method 'include?' for nil |
| P | missing-method:key | 1 | exception_introspection_accessors | uncaught exception: undefined method 'key' for an instance of KeyError |
| P | missing-method:kill | 1 | thread_kill_raise | uncaught exception: undefined method 'kill' for an instance of Thread |
| P | missing-method:lineno | 1 | io_instance_read_surface | uncaught exception: undefined method 'lineno' for an instance of File |
| P | missing-method:list | 1 | thread_list | uncaught exception: undefined method 'list' for class Thread |
| P | missing-method:match | 1 | matchdata_methods | uncaught exception: undefined method 'match' for an instance of MatchData |
| P | missing-method:members | 1 | struct_enumerable_wave8 | uncaught exception: undefined method 'members' for class Kk144 |
| P | missing-method:name | 1 | exception_value_flow_match | uncaught exception: undefined method 'name' for an instance of NoMethodError |
| P | missing-method:native_obj | 1 | native_binding_poc | uncaught exception: undefined method 'native_obj' for module NB |
| P | missing-method:open | 1 | wave_followups_2833 | uncaught exception: undefined method 'open' for class Dir |
| P | missing-method:pipe | 1 | io_pipe | uncaught exception: undefined method 'pipe' for class IO |
| P | missing-method:pipe? | 1 | file_surface_extended | uncaught exception: undefined method 'pipe?' for class File |
| P | missing-method:proc | 1 | proc_compose_curry_forward | uncaught exception: undefined method 'proc' for main |
| P | missing-method:reset | 1 | set_conformance_batch2 | uncaught exception: undefined method 'reset' for an instance of Set |
| P | missing-method:size | 1 | lazy_size | uncaught exception: undefined method 'size' for an instance of Enumerator::Lazy |
| P | missing-method:system | 1 | system_argument_list | uncaught exception: undefined method 'system' for main |
| P | missing-method:trap | 1 | signal_trap_stub | uncaught exception: undefined method 'trap' for main |
| P | missing-method:value? | 1 | env_mutation_surface | uncaught exception: undefined method 'value?' for an instance of Object |
| P | missing-method:wordy | 1 | const_aliased_class_reopen_include | uncaught exception: undefined method 'wordy' for an instance of Integer |
| k | pattern-shapes | 1 | case_in_matchdata_deconstruct | uncaught exception: no matching pattern |
| g | super-arity | 1 | reopen_split_superclass_dispatch | superclass mismatch for class Sub |

List one bucket's tests: `cargo run -p xtask -- conformance triage --bucket <name>`.
