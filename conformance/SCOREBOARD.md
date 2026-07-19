# Conformance scoreboard

Suite `spinel` — **1580/1986 passing (79.6%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `1c457a2`

| verdict | count |
|---|---|
| PASS | 1580 |
| FAIL_OUTPUT | 342 |
| FAIL_COMPILE | 56 |
| FAIL_RUSTC | 3 |
| FAIL_RUN | 1 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 4 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **1986** |

## Top failure categories

| blocked | bucket | cluster | sample test | sample message |
|---|---|---|---|---|
| 26 | spike-misc | ? | require_in_conditional | `require` is only supported as a top-level statement with a single string-literal argument (spike scope) -- it's resolved at compile time, so it can't appear inside a method, block, conditional, `begin`, or `eval` body |
| 16 | missing-method:ffi_func | P | ffi_ptr_nil | uncaught exception: undefined method 'ffi_func' for module LibC |
| 8 | unknown-class | ? | defined_guard_dead_branch | unknown class/module `MissingRoot::Sub` |
| 6 | auto-362e8305 | ? | bundle_hash | internal error: entered unreachable code: an own-only name is by definition not in captured_locals (see call.rs's split) |
| 5 | missing-method:ffi_lib | P | ffi_const_nested_module_path | uncaught exception: undefined method 'ffi_lib' for module Outer::CMath |
| 5 | splat | a | pattern_rightward_oneline | expected a `*name` splat in this array pattern (spike scope) |
| 4 | missing-method:attributes | P | compile_time_attributes | uncaught exception: undefined method 'attributes' for class CompileTimeAttributeHolder |
| 3 | arity-panic | g | str_method_nil_arg_no_segv | uncaught exception: wrong number of arguments (given 0, expected 1+) |
| 3 | auto-20dcf25f | ? | data_define_inline_receiver | `Struct.new` outside a constant assignment isn't supported (AOT: write `Name = Struct.new(:a, :b)`) |
| 3 | auto-3e862739 | ? | error_protocol_edges | attempt to take negative size (ArgumentError; spike scope: raised as a panic) |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
