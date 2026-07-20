# Gap triage

Failing tests grouped by normalized failure message, ranked by how many
tests each gap blocks. Clusters refer to the implementation plan's gap
families. Oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]`.

| cluster | bucket | blocked | sample tests | sample message |
|---|---|---|---|---|
| g | arity-panic | 4 | bundle_tiny_string, issue_3057, str_method_nil_arg_no_segv | uncaught exception: wrong number of arguments (given 2, expected 0..1) (ArgumentError) |
| P | missing-method:attributes | 4 | compile_time_attributes, compile_time_define_method_predicates, analyze_fail/attributes_non_symbol | uncaught exception: undefined method 'attributes' for class CompileTimeAttributeHolder (NoMethodError) |
| ? | auto-uncaught-signal-escaped-the-top-level-break-00b3 | 3 | issue_3024b, valued_break_proc, issue_3024 | uncaught signal escaped the top level: Break(7) |
| ? | auto-can-convert-hash-into-an-exact-number-typeerror-6187 | 2 | time_strftime_z_minimal, time_at_in_offset | uncaught exception: can't convert Hash into an exact number (TypeError) |
| ? | auto-can-iterate-from-string-typeerror-987c | 2 | bundle_tiny_num, issue_3064 | uncaught exception: can't iterate from String (TypeError) |
| ? | auto-index-too-small-for-array-minimum-indexerror-4878 | 2 | array_splice_exceptions, bundle_array_a | uncaught exception: index -7 too small for array; minimum: -3 (IndexError) |
| ? | auto-no-block-given-yield-localjumperror-1be0 | 2 | string_enum_inspect_source, enum_terminal_chunk_zip_lazy | uncaught exception: no block given (yield) (LocalJumpError) |
| P | missing-const:TCPServer | 2 | socket_tcp_thread, socket_tcp_basic | uncaught exception: uninitialized constant TCPServer (NameError) |
| P | missing-method:>= | 2 | gc_stat_string_heap, i1021 | uncaught exception: undefined method '>=' for nil (NoMethodError) |
| P | missing-method:[] | 2 | param_body_hash_inference, multi_write_call_rhs_as_expr | uncaught exception: undefined method '[]' for nil (NoMethodError) |
| P | missing-method:define_method | 2 | analyze_fail/instance_exec_def_in_block, analyze_fail/instance_exec_define_method | uncaught exception: undefined method 'define_method' for an instance of BoxPlus (NoMethodError) |
| P | missing-method:length | 2 | param_lengthlike_body_widen, bundle_misc_c_36 | uncaught exception: undefined method 'length' for nil (NoMethodError) |
| ? | auto-can-coerce-complex-into-float-typeerror-9cc0 | 1 | symbol_nil_bool_float_batch | uncaught exception: can't coerce Complex into Float (TypeError) |
| ? | auto-can-convert-time-into-an-exact-number-typeerror-0b64 | 1 | time_at_kinds_string_ctor | uncaught exception: can't convert Time into an exact number (TypeError) |
| ? | auto-can-modify-frozen-string-plain-frozenerror-b7c8 | 1 | frozen_string_literal_per_file_rev | uncaught exception: can't modify frozen String: "plain" (FrozenError) |
| ? | auto-error-unrecognized-escape-sequence-regexperror-2794 | 1 | regexp_line_anchors | error: unrecognized escape sequence (RegexpError) |
| ? | auto-expected-an-integer-got-05c6 | 1 | hash_dig | expected an Integer, got b |
| ? | auto-expected-numeric-value-got-ni-4730 | 1 | rational_complex_wave9 | expected a numeric value, got 2+3i |
| ? | auto-hash-can-be-coerced-into-integer-typeerror-8a63 | 1 | numeric_edges_wave10 | uncaught exception: Hash can't be coerced into Integer (TypeError) |
| ? | auto-infinity-floatdomainerror-feb3 | 1 | issue_3011 | uncaught exception: -Infinity (FloatDomainError) |
| ? | auto-integer-can-be-coerced-into-integer-typeerror-b387 | 1 | issue_3006 | uncaught exception: Integer can't be coerced into Integer (TypeError) |
| ? | auto-invalid-argument-argumenterror-cf9c | 1 | issue_3058 | uncaught exception: invalid argument - 1180591620717411303424 (ArgumentError) |
| ? | auto-no-implicit-conversion-of-float-into-integer-typeerror-0627 | 1 | pack_float_directives | uncaught exception: no implicit conversion of Float into Integer (TypeError) |
| ? | auto-no-implicit-conversion-of-rational-into-integer-typeerror-8b67 | 1 | time_fractional_seconds | uncaught exception: no implicit conversion of Rational into Integer (TypeError) |
| ? | auto-no-such-file-or-directory-unlink-tmp-sp-c236 | 1 | issue_3118 | uncaught exception: No such file or directory @ unlink - /tmp/sp_mkfifo_3118_39402 (Errno::ENOENT) |
| ? | auto-not-file-ioerror-6d84 | 1 | stdin_io | uncaught exception: not a file (IOError) |
| ? | auto-parsing-error-at-position-invalid-back-reference-regexperror-60c6 | 1 | i1009 | uncaught exception: Parsing error at position 5: Invalid back reference (RegexpError) |
| ? | auto-parsing-error-at-position-invalid-character-class-regexperror-db37 | 1 | regexp_encoding_introspection | uncaught exception: Parsing error at position 5: Invalid character class (RegexpError) |
| ? | auto-tried-to-create-proc-object-without-block-in-8cab | 1 | analyze_fail/instance_exec_no_block | uncaught exception: tried to create Proc object without a block (in `instance_exec') (ArgumentError) |
| ? | auto-undefined-group-name-reference-nope-123c | 1 | matchdata_named_groups | undefined group name reference: nope |
| ? | auto-wrong-argument-type-string-expected-regexp-typeerror-4d95 | 1 | string_scan_literal | uncaught exception: wrong argument type String (expected Regexp) (TypeError) |
| P | missing-const:Line | 1 | struct_block_constant_init | uncaught exception: uninitialized constant Line (NameError) |
| P | missing-const:M::C | 1 | constant_path | uncaught exception: uninitialized constant M::C (NameError) |
| P | missing-const:OpenSSL | 1 | harness_batch_2453_2456 | uncaught exception: uninitialized constant OpenSSL (NameError) |
| P | missing-const:Process::Tms | 1 | issue_3044 | uncaught exception: uninitialized constant Process::Tms (NameError) |
| P | missing-const:RequestDispatch::ViewHelpers | 1 | include_chain_module_nested_const | uncaught exception: uninitialized constant RequestDispatch::ViewHelpers (NameError) |
| P | missing-method:<< | 1 | hash_each_with_object | uncaught exception: undefined method '<<' for nil (NoMethodError) |
| P | missing-method:<= | 1 | string_split_inline_arg_gc_root | uncaught exception: undefined method '<=' for nil (NoMethodError) |
| P | missing-method:> | 1 | string_plus_heap_gc | uncaught exception: undefined method '>' for nil (NoMethodError) |
| P | missing-method:[]= | 1 | bundle_misc_c_09 | uncaught exception: undefined method '[]=' for an instance of Fiber (NoMethodError) |
| P | missing-method:attribute | 1 | compile_time_attribute_singular | uncaught exception: undefined method 'attribute' for class CompileTimeSingleAttribute (NoMethodError) |
| P | missing-method:binmode | 1 | issue_3131 | uncaught exception: undefined method 'binmode' for an instance of File (NoMethodError) |
| P | missing-method:call | 1 | proc_block_param_call | uncaught exception: undefined method 'call' for nil (NoMethodError) |
| P | missing-method:foo | 1 | poly_keyed_hash_method_dedup | uncaught exception: undefined method 'foo' for class 'Class' (NameError) |
| P | missing-method:hello | 1 | toplevel_include_module_function | uncaught exception: undefined method 'hello' for main (NoMethodError) |
| P | missing-method:hi | 1 | send_literal_and_user | uncaught exception: undefined method 'hi' for an instance of Mailer (NoMethodError) |
| P | missing-method:include? | 1 | param_include_body_widen | uncaught exception: undefined method 'include?' for nil (NoMethodError) |
| P | missing-method:kill | 1 | signal_module_surface | uncaught exception: undefined method 'kill' for module Process (NoMethodError) |
| P | missing-method:lstat | 1 | issue_2986 | uncaught exception: undefined method 'lstat' for an instance of File (NoMethodError) |
| P | missing-method:native_obj | 1 | native_binding_poc | uncaught exception: undefined method 'native_obj' for module NB (NoMethodError) |
| P | missing-method:new | 1 | basicobject_new | uncaught exception: undefined method 'new' for class BasicObject (NoMethodError) |
| P | missing-method:read | 1 | poly_keyed_hash_pipeline | uncaught exception: undefined method 'read' for class 'Class' (NameError) |
| P | missing-method:times | 1 | issue_3132 | uncaught exception: undefined method 'times' for module Process (NoMethodError) |
| P | missing-method:ungetbyte | 1 | issue_3038 | uncaught exception: undefined method 'ungetbyte' for an instance of File (NoMethodError) |
| P | missing-method:with_index | 1 | issue_2993 | uncaught exception: undefined method 'with_index' for an instance of Enumerator::Lazy (NoMethodError) |
| P | missing-method:wordy | 1 | const_aliased_class_reopen_include | uncaught exception: undefined method 'wordy' for an instance of Integer (NoMethodError) |

List one bucket's tests: `cargo run -p xtask -- conformance triage --bucket <name>`.
