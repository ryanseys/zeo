# Conformance scoreboard

Suite `spinel` — **2312/2336 passing (99.0%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25] [--disable-error_highlight --disable-did_you_mean]` — zeo `69f0c9d`

| verdict | count |
|---|---|
| PASS | 2312 |
| FAIL_OUTPUT | 23 |
| FAIL_COMPILE | 1 |
| FAIL_RUSTC | 0 |
| FAIL_RUN | 0 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 0 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **2336** |

## Top failure categories

| blocked | bucket | cluster | sample test | sample message |
|---|---|---|---|---|
| 1 | arity-panic | g | str_method_nil_arg_no_segv | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/str_method_nil_arg_no_segv.rb:19:in '<main>': wrong number of arguments (given 0, expected 1+) (ArgumentError) |
| 1 | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-2467 | ? | hash_each_with_object | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/hash_each_with_object.rb:15:in '<main>' |
| 1 | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-4add | ? | instance_exec_def_singleton | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/instance_exec_def_singleton.rb:11:in '<main>' |
| 1 | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-7011 | ? | frozen_string_literal_per_file_rev | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/frozen_string_literal_per_file_rev.rb:8:in '<main>' |
| 1 | auto-from-users-ryanseys-dev-zeo-crates-xtask-conformance-e249 | ? | ffi_callback | from /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/ffi_callback.rb:24:in '<main>' |
| 1 | auto-territory-not-syntaxerror-ed71 | ? | ffi_variadic | territory, not a SyntaxError) |
| 1 | auto-users-ryanseys-dev-zeo-crates-xtask-conformance-corpus-cac9 | ? | socket_tcp_thread | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/socket_tcp_thread.rb:14:in '<main>': Unknown error @ read -  (SystemCallError) |
| 1 | missing-const:Line | P | struct_block_constant_init | uninitialized constant Line (NameError) |
| 1 | missing-const:M::C | P | constant_path | uninitialized constant M::C (NameError) |
| 1 | missing-method:hello | P | toplevel_include_module_function | /Users/ryanseys/dev/zeo/crates/xtask/../../conformance/corpus/test/toplevel_include_module_function.rb:20:in '<main>': undefined method 'hello' for main (NoMethodError) |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
