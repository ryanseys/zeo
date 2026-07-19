# Conformance scoreboard

Suite `rubyspec` — **4/67 passing (6.0%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `befeb9a`

| verdict | count |
|---|---|
| PASS | 4 |
| FAIL_OUTPUT | 9 |
| FAIL_COMPILE | 48 |
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
| 35 | spike-misc | ? | break_spec | a nested escaping block capturing its enclosing BLOCK's own local `__f1_value` isn't supported yet (spike scope) -- move it to the enclosing method/top level, which makes it a shared Captured cell |
| 6 | missing-method:raise | P | numbers_spec | ERROR: A number literal must have a digit before the decimal point: NoMethodError: undefined method 'raise' for an instance of MSpecShould |
| 2 | missing-const:ScratchPad | P | BEGIN_spec | ERROR: The BEGIN keyword returns the top-level script's filename for __FILE__: NameError: uninitialized constant ScratchPad |
| 2 | rustc-failure | ! | yield_spec | rustc failed compiling the generated program (source at /var/folders/7r/0kdzlwm12_19f3qc1j8w5vjr0000gn/T/spinelc-gen-7597d00610a0a925.rs) |
| 2 | splat | a | variables_spec | /Users/ryanseys/dev/spec/language/variables_spec.rb: expected `*name` as a multi-assignment's splat target |
| 1 | auto-619a71ee | ? | block_spec | /Users/ryanseys/dev/spec/language/block_spec.rb: unsupported syntax at "\"a\" => 1, a: 10" (spike handles only what the 7 example programs need) |
| 1 | auto-72ea658f | ? | module_spec | /Users/ryanseys/dev/spec/language/module_spec.rb: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| 1 | auto-80fe6faf | ? | execution_spec | /Users/ryanseys/dev/spec/language/execution_spec.rb: unsupported syntax at "`echo disc #{ip}`" (spike handles only what the 7 example programs need) |
| 1 | auto-8576fc82 | ? | encoding_spec | uncaught exception: eval: unsupported syntax in this build (the eval VM does not yet cover this node) |
| 1 | auto-ba2b7615 | ? | precedence_spec | /Users/ryanseys/dev/spec/language/precedence_spec.rb: unsupported syntax at "from..to" (spike handles only what the 7 example programs need) |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
