# Conformance scoreboard

Suite `rubyspec` — **5/67 passing (7.5%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `b54ec9f`

| verdict | count |
|---|---|
| PASS | 5 |
| FAIL_OUTPUT | 10 |
| FAIL_COMPILE | 46 |
| FAIL_RUSTC | 2 |
| FAIL_RUN | 4 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 0 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **67** |

## Top failure categories

| blocked | bucket | cluster | sample test | sample message |
|---|---|---|---|---|
| 29 | spike-misc | ? | file_spec | /Users/ryanseys/dev/spec/language/file_spec.rb: /Users/ryanseys/dev/spec/fixtures/code_loading.rb: `require` is only supported as a top-level statement with a single string-literal argument (spike scope) -- it's resolved at compile time, so it can't appear inside a method, block, conditional, `begin`, or `eval` body |
| 7 | missing-method:raise | P | retry_spec | ERROR: The retry statement raises a SyntaxError when used outside of a rescue statement: NoMethodError: undefined method 'raise' for an instance of MSpecShould |
| 2 | missing-const:ScratchPad | P | BEGIN_spec | ERROR: The BEGIN keyword returns the top-level script's filename for __FILE__: NameError: uninitialized constant ScratchPad |
| 2 | rustc-failure | ! | yield_spec | rustc failed compiling the generated program (source at /var/folders/7r/0kdzlwm12_19f3qc1j8w5vjr0000gn/T/spinelc-gen-ccd93bfc2d30cf73.rs) |
| 2 | splat | a | variables_spec | /Users/ryanseys/dev/spec/language/variables_spec.rb: expected `*name` as a multi-assignment's splat target |
| 2 | toplevel-node-in-expr | ? | private_spec | unexpected top-level-only node in expression position |
| 1 | auto-619a71ee | ? | block_spec | /Users/ryanseys/dev/spec/language/block_spec.rb: unsupported syntax at "\"a\" => 1, a: 10" (spike handles only what the 7 example programs need) |
| 1 | auto-6f3c779d | ? | if_spec | /Users/ryanseys/dev/spec/language/if_spec.rb: unsupported syntax at "(i == 4)..(i == 4)" (spike handles only what the 7 example programs need) |
| 1 | auto-72ea658f | ? | module_spec | /Users/ryanseys/dev/spec/language/module_spec.rb: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| 1 | auto-792f6bff | ? | method_spec | /Users/ryanseys/dev/spec/language/method_spec.rb: unsupported syntax at "..." (spike handles only what the 7 example programs need) |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
