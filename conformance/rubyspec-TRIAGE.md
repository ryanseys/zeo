# Gap triage

Failing tests grouped by normalized failure message, ranked by how many
tests each gap blocks. Clusters refer to the implementation plan's gap
families. Oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]`.

| cluster | bucket | blocked | sample tests | sample message |
|---|---|---|---|---|
| ? | spike-misc | 35 | break_spec, keyword_arguments_spec, regexp_spec | a nested escaping block capturing its enclosing BLOCK's own local `__f1_value` isn't supported yet (spike scope) -- move it to the enclosing method/top level, which makes it a shared Captured cell |
| P | missing-method:raise | 6 | numbers_spec, retry_spec, delegation_spec | ERROR: A number literal must have a digit before the decimal point: NoMethodError: undefined method 'raise' for an instance of MSpecShould |
| P | missing-const:ScratchPad | 2 | BEGIN_spec, symbol_spec | ERROR: The BEGIN keyword returns the top-level script's filename for __FILE__: NameError: uninitialized constant ScratchPad |
| ! | rustc-failure | 2 | yield_spec, order_spec | rustc failed compiling the generated program (source at /var/folders/7r/0kdzlwm12_19f3qc1j8w5vjr0000gn/T/spinelc-gen-ccd93bfc2d30cf73.rs) |
| a | splat | 2 | variables_spec, for_spec | /Users/ryanseys/dev/spec/language/variables_spec.rb: expected `*name` as a multi-assignment's splat target |
| ? | auto-619a71ee | 1 | block_spec | /Users/ryanseys/dev/spec/language/block_spec.rb: unsupported syntax at "\"a\" => 1, a: 10" (spike handles only what the 7 example programs need) |
| ? | auto-72ea658f | 1 | module_spec | /Users/ryanseys/dev/spec/language/module_spec.rb: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-80fe6faf | 1 | execution_spec | /Users/ryanseys/dev/spec/language/execution_spec.rb: unsupported syntax at "`echo disc #{ip}`" (spike handles only what the 7 example programs need) |
| ? | auto-8576fc82 | 1 | encoding_spec | uncaught exception: eval: unsupported syntax in this build (the eval VM does not yet cover this node) |
| ? | auto-ba2b7615 | 1 | precedence_spec | /Users/ryanseys/dev/spec/language/precedence_spec.rb: unsupported syntax at "from..to" (spike handles only what the 7 example programs need) |
| ? | auto-bcd16e8b | 1 | assignments_spec | /Users/ryanseys/dev/spec/language/assignments_spec.rb: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-bf7964ee | 1 | undef_spec | /Users/ryanseys/dev/spec/language/undef_spec.rb: unsupported syntax at "undef meth" (spike handles only what the 7 example programs need) |
| ? | auto-e0dfb590 | 1 | defined_spec | /Users/ryanseys/dev/spec/language/defined_spec.rb: /Users/ryanseys/dev/spec/language/fixtures/defined.rb: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-f0c6b32f | 1 | END_spec | uncaught exception: it_behaves_like |
| c | compound-assign | 1 | optional_assignments_spec | /Users/ryanseys/dev/spec/language/optional_assignments_spec.rb: `[]`-style compound assignment only supports a single index argument (spike scope) |
| P | missing-const:MSpecLite::RUBY_VERSION | 1 | numbered_parameters_spec | uncaught exception: uninitialized constant MSpecLite::RUBY_VERSION |
| P | missing-method:define_method | 1 | string_spec | uncaught exception: undefined method 'define_method' for an instance of Object |
| P | missing-method:new | 1 | range_spec | ERROR: Literal Ranges creates beginless ranges: NoMethodError: undefined method 'new' for class Range |
| k | pattern-shapes | 1 | pattern_matching_spec | /Users/ryanseys/dev/spec/language/pattern_matching_spec.rb: a pattern can't bind a variable inside a `\|` alternation (spike scope, matches real Ruby) |
| g | super-arity | 1 | super_spec | /Users/ryanseys/dev/spec/language/super_spec.rb: /Users/ryanseys/dev/spec/language/fixtures/super.rb: unsupported statement in `class << self` (spike scope) -- only `def`s, constants, `include`, and `attr_*`/`private`/`alias` are handled here; `extend`/`prepend`/ivars/a nested `class << self` aren't supported yet |
| ? | toplevel-node-in-expr | 1 | private_spec | unexpected top-level-only node in expression position |

List one bucket's tests: `cargo run -p xtask -- conformance triage --bucket <name>`.
