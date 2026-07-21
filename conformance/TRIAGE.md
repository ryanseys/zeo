# Gap triage

Failing tests grouped by normalized failure message, ranked by how many
tests each gap blocks. Clusters refer to the implementation plan's gap
families. Oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25] [--disable-error_highlight --disable-did_you_mean]`.

| cluster | bucket | blocked | sample tests | sample message |
|---|---|---|---|---|
| ? | auto-cannot-load-such-file-ostruct-0485 | 5 | issue_3193, issue_3194, issue_3197 | cannot load such file -- ostruct |
| g | arity-panic | 4 | bundle_tiny_string, issue_3057, str_method_nil_arg_no_segv | uncaught exception: wrong number of arguments (given 2, expected 0..1) (ArgumentError) |
| P | missing-method:attributes | 4 | compile_time_attributes, compile_time_define_method_predicates, analyze_fail/attributes_non_symbol | uncaught exception: undefined method 'attributes' for class CompileTimeAttributeHolder (NoMethodError) |
| ? | auto-index-too-small-for-array-minimum-indexerror-4878 | 2 | array_splice_exceptions, bundle_array_a | uncaught exception: index -7 too small for array; minimum: -3 (IndexError) |
| ? | auto-no-block-given-yield-localjumperror-1be0 | 2 | string_enum_inspect_source, enum_terminal_chunk_zip_lazy | uncaught exception: no block given (yield) (LocalJumpError) |
| P | missing-method:>= | 2 | gc_stat_string_heap, i1021 | uncaught exception: undefined method '>=' for nil (NoMethodError) |
| P | missing-method:define_method | 2 | analyze_fail/instance_exec_def_in_block, analyze_fail/instance_exec_define_method | uncaught exception: undefined method 'define_method' for an instance of BoxPlus (NoMethodError) |
| P | missing-method:length | 2 | param_lengthlike_body_widen, bundle_misc_c_36 | uncaught exception: undefined method 'length' for nil (NoMethodError) |
| ? | auto-can-coerce-complex-into-float-typeerror-9cc0 | 1 | symbol_nil_bool_float_batch | uncaught exception: can't coerce Complex into Float (TypeError) |
| ? | auto-can-modify-frozen-string-plain-frozenerror-b7c8 | 1 | frozen_string_literal_per_file_rev | uncaught exception: can't modify frozen String: "plain" (FrozenError) |
| ? | auto-error-unrecognized-escape-sequence-regexperror-2794 | 1 | regexp_line_anchors | error: unrecognized escape sequence (RegexpError) |
| ? | auto-expected-numeric-value-got-ni-4730 | 1 | rational_complex_wave9 | expected a numeric value, got 2+3i |
| ? | auto-hash-can-be-coerced-into-integer-typeerror-8a63 | 1 | numeric_edges_wave10 | uncaught exception: Hash can't be coerced into Integer (TypeError) |
| ? | auto-integer-does-not-have-dig-method-typeerror-ee2a | 1 | hash_dig | uncaught exception: Integer does not have #dig method (TypeError) |
| ? | auto-no-receiver-is-available-argumenterror-234c | 1 | issue_3119 | uncaught exception: no receiver is available (ArgumentError) |
| ? | auto-tried-to-create-proc-object-without-block-in-8cab | 1 | analyze_fail/instance_exec_no_block | uncaught exception: tried to create Proc object without a block (in `instance_exec') (ArgumentError) |
| P | missing-const:A | 1 | issue_3179 | uncaught exception: uninitialized constant A (NameError) |
| P | missing-const:Line | 1 | struct_block_constant_init | uncaught exception: uninitialized constant Line (NameError) |
| P | missing-const:M::C | 1 | constant_path | uncaught exception: uninitialized constant M::C (NameError) |
| P | missing-const:OpenSSL | 1 | harness_batch_2453_2456 | uncaught exception: uninitialized constant OpenSSL (NameError) |
| P | missing-const:RequestDispatch::ViewHelpers | 1 | include_chain_module_nested_const | uncaught exception: uninitialized constant RequestDispatch::ViewHelpers (NameError) |
| P | missing-const:User | 1 | issue_3180 | uncaught exception: uninitialized constant User (NameError) |
| P | missing-method:<< | 1 | hash_each_with_object | uncaught exception: undefined method '<<' for nil (NoMethodError) |
| P | missing-method:<= | 1 | string_split_inline_arg_gc_root | uncaught exception: undefined method '<=' for nil (NoMethodError) |
| P | missing-method:> | 1 | string_plus_heap_gc | uncaught exception: undefined method '>' for nil (NoMethodError) |
| P | missing-method:[] | 1 | param_body_hash_inference | uncaught exception: undefined method '[]' for nil (NoMethodError) |
| P | missing-method:[]= | 1 | bundle_misc_c_09 | uncaught exception: undefined method '[]=' for an instance of Fiber (NoMethodError) |
| P | missing-method:attribute | 1 | compile_time_attribute_singular | uncaught exception: undefined method 'attribute' for class CompileTimeSingleAttribute (NoMethodError) |
| P | missing-method:foo | 1 | poly_keyed_hash_method_dedup | uncaught exception: undefined method 'foo' for class 'Class' (NameError) |
| P | missing-method:hello | 1 | toplevel_include_module_function | uncaught exception: undefined method 'hello' for main (NoMethodError) |
| P | missing-method:hi | 1 | send_literal_and_user | uncaught exception: undefined method 'hi' for an instance of Mailer (NoMethodError) |
| P | missing-method:include? | 1 | param_include_body_widen | uncaught exception: undefined method 'include?' for nil (NoMethodError) |
| P | missing-method:kill | 1 | signal_module_surface | uncaught exception: undefined method 'kill' for module Process (NoMethodError) |
| P | missing-method:native_obj | 1 | native_binding_poc | uncaught exception: undefined method 'native_obj' for module NB (NoMethodError) |
| P | missing-method:new | 1 | basicobject_new | uncaught exception: undefined method 'new' for class BasicObject (NoMethodError) |
| P | missing-method:read | 1 | poly_keyed_hash_pipeline | uncaught exception: undefined method 'read' for class 'Class' (NameError) |
| P | missing-method:with_index | 1 | issue_2993 | uncaught exception: undefined method 'with_index' for an instance of Enumerator::Lazy (NoMethodError) |
| P | missing-method:wordy | 1 | const_aliased_class_reopen_include | uncaught exception: undefined method 'wordy' for an instance of Integer (NoMethodError) |

List one bucket's tests: `cargo run -p xtask -- conformance triage --bucket <name>`.
