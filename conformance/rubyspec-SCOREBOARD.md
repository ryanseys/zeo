# Conformance scoreboard

Suite `rubyspec` — **5/67 passing (7.5%)** — oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]` — spinel-rs `b55c430`

| verdict | count |
|---|---|
| PASS | 5 |
| FAIL_OUTPUT | 11 |
| FAIL_COMPILE | 47 |
| FAIL_RUSTC | 1 |
| FAIL_RUN | 3 |
| TIMEOUT_COMPILE | 0 |
| TIMEOUT_RUN | 0 |
| ORACLE_FAIL | 0 |
| SKIP | 0 |
| **TOTAL** | **67** |

## Top failure categories

| blocked | bucket | cluster | sample test | sample message |
|---|---|---|---|---|
| 31 | spike-misc | ? | metaclass_spec | /Users/ryanseys/dev/spec/language/metaclass_spec.rb: /Users/ryanseys/dev/spec/fixtures/class.rb: `class << self` at this position isn't supported yet (spike scope) -- use it inside a class/module body |
| 7 | missing-method:raise | P | retry_spec | ERROR: The retry statement raises a SyntaxError when used outside of a rescue statement: NoMethodError: undefined method 'raise' for an instance of MSpecShould |
| 2 | missing-const:ScratchPad | P | BEGIN_spec | ERROR: The BEGIN keyword returns the top-level script's filename for __FILE__: NameError: uninitialized constant ScratchPad |
| 2 | toplevel-node-in-expr | ? | private_spec | unexpected top-level-only node in expression position |
| 1 | auto-eval-unsupported-syntax-in-this-build-the-eval-fc82 | ? | encoding_spec | uncaught exception: eval: unsupported syntax in this build (the eval VM does not yet cover this node) |
| 1 | auto-fail-method-call-evaluates-block-pass-after-receiver-1192 | ? | order_spec | FAIL: A method call evaluates block pass after receiver: expected [nil] == [false] |
| 1 | auto-it-behaves-like-b32f | ? | END_spec | uncaught exception: it_behaves_like |
| 1 | auto-users-ryanseys-dev-spec-language-array-spec-rb-e598 | ? | array_spec | /Users/ryanseys/dev/spec/language/array_spec.rb: unsupported syntax at "\"foo\" => :bar, baz: 42" (spike handles only what the 7 example programs need) |
| 1 | auto-users-ryanseys-dev-spec-language-assignments-spec-rb-6e8b | ? | assignments_spec | /Users/ryanseys/dev/spec/language/assignments_spec.rb: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| 1 | auto-users-ryanseys-dev-spec-language-block-spec-rb-71ee | ? | block_spec | /Users/ryanseys/dev/spec/language/block_spec.rb: unsupported syntax at "\"a\" => 1, a: 10" (spike handles only what the 7 example programs need) |

## Skipped tests

(none)

Regenerate with `cargo run -p xtask -- conformance run --update-scoreboard`.
