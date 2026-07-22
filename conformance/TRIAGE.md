# Gap triage

Failing tests grouped by normalized failure message, ranked by how many
tests each gap blocks. Clusters refer to the implementation plan's gap
families. Oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25] [--disable-error_highlight --disable-did_you_mean]`.

| cluster | bucket | blocked | sample tests | sample message |
|---|---|---|---|---|
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-a2be | 5 | issue_3193, issue_3194, issue_3197 | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/issue_3193.rb:109:in '<main>' |
| g | arity-panic | 3 | str_method_nil_arg_no_segv, string_enum_arg_forms, issue_3057 | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/str_method_nil_arg_no_segv.rb:19:in '<main>': wrong number of arguments (given 0, expected 1+) (ArgumentError) |
| ? | auto-users-ryanseys-dev-zeo-crates-xtask-conformance-corpus-4d1b | 2 | issue_3119, issue_3002 | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/issue_3119.rb:8:in '<main>': no receiver is available (ArgumentError) |
| P | missing-method:>= | 2 | gc_stat_string_heap, i1021 | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/gc_stat_string_heap.rb:13:in '<main>': undefined method '>=' for nil (NoMethodError) |
| ? | auto-error-unrecognized-escape-sequence-regexperror-2794 | 1 | regexp_line_anchors | error: unrecognized escape sequence (RegexpError) |
| ? | auto-expected-numeric-value-got-ni-4730 | 1 | rational_complex_wave9 | expected a numeric value, got 2+3i |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-01c0 | 1 | compile_time_attribute_singular | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/compile_time_attribute_singular.rb:1:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-0827 | 1 | string_split_inline_arg_gc_root | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/string_split_inline_arg_gc_root.rb:43:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-1c7a | 1 | param_lengthlike_body_widen | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/param_lengthlike_body_widen.rb:44:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-2467 | 1 | hash_each_with_object | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/hash_each_with_object.rb:15:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-26e1 | 1 | param_body_hash_inference | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/param_body_hash_inference.rb:51:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-27f6 | 1 | bundle_misc_c_09 | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/bundle_misc_c_09.rb:36:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-3ae3 | 1 | bundle_tiny_string | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/bundle_tiny_string.rb:166:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-4add | 1 | instance_exec_def_singleton | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/instance_exec_def_singleton.rb:11:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-59e4 | 1 | native_binding_poc | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/native_binding_poc.rb:5:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-5ed7 | 1 | compile_time_attributes | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/compile_time_attributes.rb:1:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-7011 | 1 | frozen_string_literal_per_file_rev | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/frozen_string_literal_per_file_rev.rb:8:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-823a | 1 | compile_time_attribute_wrapped_record | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/compile_time_attribute_wrapped_record.rb:3:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-ca26 | 1 | bundle_array_a | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/bundle_array_a.rb:193:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-d498 | 1 | compile_time_define_method_predicates | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/compile_time_define_method_predicates.rb:1:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-e249 | 1 | ffi_callback | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/ffi_callback.rb:24:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-faa9 | 1 | param_include_body_widen | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/param_include_body_widen.rb:24:in '<main>' |
| ? | auto-territory-not-syntaxerror-ed71 | 1 | ffi_variadic | territory, not a SyntaxError) |
| ? | auto-users-ryanseys-dev-zeo-crates-xtask-conformance-corpus-10d6 | 1 | hash_dig | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/hash_dig.rb:72:in '<main>': Integer does not have #dig method (TypeError) |
| ? | auto-users-ryanseys-dev-zeo-crates-xtask-conformance-corpus-1c66 | 1 | array_splice_exceptions | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/array_splice_exceptions.rb:55:in '<main>': index -7 too small for array; minimum: -3 (IndexError) |
| ? | auto-users-ryanseys-dev-zeo-crates-xtask-conformance-corpus-84cc | 1 | symbol_nil_bool_float_batch | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/symbol_nil_bool_float_batch.rb:61:in '<main>': can't coerce Complex into Float (TypeError) |
| ? | auto-users-ryanseys-dev-zeo-crates-xtask-conformance-corpus-9fab | 1 | string_enum_inspect_source | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/string_enum_inspect_source.rb:8:in '<main>': no block given (yield) (LocalJumpError) |
| ? | auto-users-ryanseys-dev-zeo-crates-xtask-conformance-corpus-a92a | 1 | numeric_edges_wave10 | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/numeric_edges_wave10.rb:11:in '<main>': Hash can't be coerced into Integer (TypeError) |
| ? | auto-users-ryanseys-dev-zeo-crates-xtask-conformance-corpus-c5a2 | 1 | constant_path | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/constant_path.rb:29:in '<main>': tried to create Proc object without a block (ArgumentError) |
| ? | auto-users-ryanseys-dev-zeo-crates-xtask-conformance-corpus-d260 | 1 | enum_terminal_chunk_zip_lazy | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/enum_terminal_chunk_zip_lazy.rb:3:in '<main>': no block given (yield) (LocalJumpError) |
| P | missing-const:A | 1 | issue_3179 | uninitialized constant A (NameError) |
| P | missing-const:Line | 1 | struct_block_constant_init | uninitialized constant Line (NameError) |
| P | missing-const:OpenSSL | 1 | harness_batch_2453_2456 | uninitialized constant OpenSSL (NameError) |
| P | missing-const:RequestDispatch::ViewHelpers | 1 | include_chain_module_nested_const | uninitialized constant RequestDispatch::ViewHelpers (NameError) |
| P | missing-const:User | 1 | issue_3180 | uninitialized constant User (NameError) |
| P | missing-method:> | 1 | string_plus_heap_gc | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/string_plus_heap_gc.rb:20:in '<main>': undefined method '>' for nil (NoMethodError) |
| P | missing-method:hello | 1 | toplevel_include_module_function | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/toplevel_include_module_function.rb:20:in '<main>': undefined method 'hello' for main (NoMethodError) |
| P | missing-method:hi | 1 | send_literal_and_user | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/send_literal_and_user.rb:27:in '<main>': undefined method 'hi' for an instance of Mailer (NoMethodError) |
| P | missing-method:kill | 1 | signal_module_surface | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/signal_module_surface.rb:33:in '<main>': undefined method 'kill' for module Process (NoMethodError) |
| P | missing-method:new | 1 | basicobject_new | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/basicobject_new.rb:3:in '<main>': undefined method 'new' for class BasicObject (NoMethodError) |
| P | missing-method:with_index | 1 | issue_2993 | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/issue_2993.rb:3:in '<main>': undefined method 'with_index' for an instance of Enumerator::Lazy (NoMethodError) |
| P | missing-method:wordy | 1 | const_aliased_class_reopen_include | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/const_aliased_class_reopen_include.rb:18:in '<main>': undefined method 'wordy' for an instance of Integer (NoMethodError) |

List one bucket's tests: `cargo run -p xtask -- conformance triage --bucket <name>`.
