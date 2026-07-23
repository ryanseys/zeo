# Gap triage

Failing tests grouped by normalized failure message, ranked by how many
tests each gap blocks. Clusters refer to the implementation plan's gap
families. Oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25] [--disable-error_highlight --disable-did_you_mean]`.

| cluster | bucket | blocked | sample tests | sample message |
|---|---|---|---|---|
| g | arity-panic | 1 | str_method_nil_arg_no_segv | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/str_method_nil_arg_no_segv.rb:19:in '<main>': wrong number of arguments (given 0, expected 1+) (ArgumentError) |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-2467 | 1 | hash_each_with_object | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/hash_each_with_object.rb:15:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-4add | 1 | instance_exec_def_singleton | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/instance_exec_def_singleton.rb:11:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-7011 | 1 | frozen_string_literal_per_file_rev | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/frozen_string_literal_per_file_rev.rb:8:in '<main>' |
| ? | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-e249 | 1 | ffi_callback | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/ffi_callback.rb:24:in '<main>' |
| ? | auto-territory-not-syntaxerror-ed71 | 1 | ffi_variadic | territory, not a SyntaxError) |
| ? | auto-users-ryanseys-dev-zeo-crates-xtask-conformance-corpus-cac9 | 1 | socket_tcp_thread | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/socket_tcp_thread.rb:14:in '<main>': Unknown error @ read -  (SystemCallError) |
| P | missing-const:Line | 1 | struct_block_constant_init | uninitialized constant Line (NameError) |
| P | missing-const:M::C | 1 | constant_path | uninitialized constant M::C (NameError) |
| P | missing-method:hello | 1 | toplevel_include_module_function | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/toplevel_include_module_function.rb:20:in '<main>': undefined method 'hello' for main (NoMethodError) |
| P | missing-method:hi | 1 | send_literal_and_user | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/send_literal_and_user.rb:27:in '<main>': undefined method 'hi' for an instance of Mailer (NoMethodError) |
| P | missing-method:wordy | 1 | const_aliased_class_reopen_include | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/const_aliased_class_reopen_include.rb:18:in '<main>': undefined method 'wordy' for an instance of Integer (NoMethodError) |

List one bucket's tests: `cargo run -p xtask -- conformance triage --bucket <name>`.
