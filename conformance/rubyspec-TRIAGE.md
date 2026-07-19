# Gap triage

Failing tests grouped by normalized failure message, ranked by how many
tests each gap blocks. Clusters refer to the implementation plan's gap
families. Oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]`.

| cluster | bucket | blocked | sample tests | sample message |
|---|---|---|---|---|
| ? | spike-misc | 29 | file_spec, metaclass_spec, source_encoding_spec | /Users/ryanseys/dev/spec/language/file_spec.rb: /Users/ryanseys/dev/spec/fixtures/code_loading.rb: `require` is only supported as a top-level statement with a single string-literal argument (spike scope) -- it's resolved at compile time, so it can't appear inside a method, block, conditional, `begin`, or `eval` body |
| P | missing-method:raise | 7 | retry_spec, delegation_spec, numbers_spec | ERROR: The retry statement raises a SyntaxError when used outside of a rescue statement: NoMethodError: undefined method 'raise' for an instance of MSpecShould |
| P | missing-const:ScratchPad | 2 | BEGIN_spec, symbol_spec | ERROR: The BEGIN keyword returns the top-level script's filename for __FILE__: NameError: uninitialized constant ScratchPad |
| ? | toplevel-node-in-expr | 2 | private_spec, next_spec | internal error: unexpected top-level-only node in expression position |
| ? | auto-eval-unsupported-syntax-in-this-build-the-eval-fc82 | 1 | encoding_spec | uncaught exception: eval: unsupported syntax in this build (the eval VM does not yet cover this node) |
| ? | auto-fail-the-until-modifier-with-begin-end-block-c646 | 1 | until_spec | FAIL: The until modifier with begin .. end block restart the current iteration without reevaluating condition with redo: expected [1, 1, 1, 2] == [0, 0, 0, 1, 2] |
| ? | auto-fail-the-while-modifier-with-begin-end-block-6764 | 1 | while_spec | FAIL: The while modifier with begin .. end block restarts the current iteration without reevaluating condition with redo: expected [1, 1, 1, 2] == [0, 0, 0, 1, 2] |
| ? | auto-it-behaves-like-b32f | 1 | END_spec | uncaught exception: it_behaves_like |
| ? | auto-users-ryanseys-dev-spec-language-array-spec-rb-e598 | 1 | array_spec | /Users/ryanseys/dev/spec/language/array_spec.rb: unsupported syntax at "\"foo\" => :bar, baz: 42" (spike handles only what the 7 example programs need) |
| ? | auto-users-ryanseys-dev-spec-language-assignments-spec-rb-6e8b | 1 | assignments_spec | /Users/ryanseys/dev/spec/language/assignments_spec.rb: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-users-ryanseys-dev-spec-language-block-spec-rb-71ee | 1 | block_spec | /Users/ryanseys/dev/spec/language/block_spec.rb: unsupported syntax at "\"a\" => 1, a: 10" (spike handles only what the 7 example programs need) |
| ? | auto-users-ryanseys-dev-spec-language-defined-spec-rb-b590 | 1 | defined_spec | /Users/ryanseys/dev/spec/language/defined_spec.rb: /Users/ryanseys/dev/spec/language/fixtures/defined.rb: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-users-ryanseys-dev-spec-language-execution-spec-rb-6faf | 1 | execution_spec | /Users/ryanseys/dev/spec/language/execution_spec.rb: unsupported syntax at "`echo disc #{ip}`" (spike handles only what the 7 example programs need) |
| ? | auto-users-ryanseys-dev-spec-language-if-spec-rb-779d | 1 | if_spec | /Users/ryanseys/dev/spec/language/if_spec.rb: unsupported syntax at "(i == 4)..(i == 4)" (spike handles only what the 7 example programs need) |
| ? | auto-users-ryanseys-dev-spec-language-method-spec-rb-6bff | 1 | method_spec | /Users/ryanseys/dev/spec/language/method_spec.rb: unsupported syntax at "..." (spike handles only what the 7 example programs need) |
| ? | auto-users-ryanseys-dev-spec-language-module-spec-rb-658f | 1 | module_spec | /Users/ryanseys/dev/spec/language/module_spec.rb: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-users-ryanseys-dev-spec-language-precedence-spec-rb-7615 | 1 | precedence_spec | /Users/ryanseys/dev/spec/language/precedence_spec.rb: unsupported syntax at "from..to" (spike handles only what the 7 example programs need) |
| ? | auto-users-ryanseys-dev-spec-language-undef-spec-rb-64ee | 1 | undef_spec | /Users/ryanseys/dev/spec/language/undef_spec.rb: unsupported syntax at "undef meth" (spike handles only what the 7 example programs need) |
| c | compound-assign | 1 | optional_assignments_spec | /Users/ryanseys/dev/spec/language/optional_assignments_spec.rb: `[]`-style compound assignment only supports a single index argument (spike scope) |
| P | missing-const:MSpecLite::RUBY_VERSION | 1 | numbered_parameters_spec | uncaught exception: uninitialized constant MSpecLite::RUBY_VERSION |
| P | missing-method:new | 1 | range_spec | ERROR: Literal Ranges creates beginless ranges: NoMethodError: undefined method 'new' for class Range |
| k | pattern-shapes | 1 | pattern_matching_spec | /Users/ryanseys/dev/spec/language/pattern_matching_spec.rb: a pattern can't bind a variable inside a `\|` alternation (spike scope, matches real Ruby) |
| ! | rustc-failure | 1 | yield_spec | rustc failed compiling the generated program (source at /var/folders/7r/0kdzlwm12_19f3qc1j8w5vjr0000gn/T/spinelc-gen-f4ea6f5f2e44bd17.rs) |
| a | splat | 1 | for_spec | /Users/ryanseys/dev/spec/language/for_spec.rb: expected `*name` as a multi-assignment's splat target |
| g | super-arity | 1 | super_spec | /Users/ryanseys/dev/spec/language/super_spec.rb: /Users/ryanseys/dev/spec/language/fixtures/super.rb: unsupported statement in `class << self` (spike scope) -- only `def`s, constants, `include`, and `attr_*`/`private`/`alias` are handled here; `extend`/`prepend`/ivars/a nested `class << self` aren't supported yet |

List one bucket's tests: `cargo run -p xtask -- conformance triage --bucket <name>`.
