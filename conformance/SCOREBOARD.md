# Conformance scoreboard

Suite `spinel` — **1663/2067 passing (80.5%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `105d20c`

| verdict | count |
|---|---|
| PASS | 1663 |
| FAIL_OUTPUT | 350 |
| FAIL_COMPILE | 46 |
| FAIL_RUSTC | 3 |
| FAIL_RUN | 1 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 4 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **2067** |

## Top failure categories

| blocked | bucket | cluster | sample test | sample message |
|---|---|---|---|---|
| 28 | spike-misc | ? | singleton_method_object | a singleton method (`def obj.name`) that uses `yield`/`block_given?`/`&block` isn't supported yet (spike scope) -- the method's own block isn't threaded through the runtime install |
| 16 | missing-method:ffi_func | P | ffi_ptr_nil | uncaught exception: undefined method 'ffi_func' for module LibC |
| 6 | unknown-class | ? | harness_batch_2453_2456 | unknown class/module `OpenSSL` |
| 5 | missing-method:ffi_lib | P | ffi_const_nested_module_path | uncaught exception: undefined method 'ffi_lib' for module Outer::CMath |
| 4 | missing-method:attributes | P | compile_time_attributes | uncaught exception: undefined method 'attributes' for class CompileTimeAttributeHolder |
| 4 | missing-method:new | P | issue_2968 | uncaught exception: undefined method 'new' for class Dir |
| 3 | arity-panic | g | str_method_nil_arg_no_segv | uncaught exception: wrong number of arguments (given 0, expected 1+) |
| 3 | auto-20dcf25f | ? | data_define_inline_receiver | `Struct.new` outside a constant assignment isn't supported (AOT: write `Name = Struct.new(:a, :b)`) |
| 3 | auto-3e862739 | ? | error_protocol_edges | attempt to take negative size (ArgumentError; spike scope: raised as a panic) |
| 3 | missing-method:+ | P | enumerator_ops | uncaught exception: undefined method '+' for an instance of Enumerator |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
