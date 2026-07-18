# Gap triage

Failing tests grouped by normalized failure message, ranked by how many
tests each gap blocks. Clusters refer to the implementation plan's gap
families. Oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]`.

| cluster | bucket | blocked | sample test | sample message |
|---|---|---|---|---|
| ? | spike-misc | 49 | BEGIN_spec | note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace |
| a | splat | 2 | for_spec | spinelc: /Users/ryanseys/dev/spec/language/for_spec.rb: expected `*name` as a multi-assignment's splat target |
| ? | auto-619a71ee | 1 | block_spec | spinelc: /Users/ryanseys/dev/spec/language/block_spec.rb: unsupported syntax at "\"a\" => 1, a: 10" (spike handles only what the 7 example programs need) |
| ? | auto-72ea658f | 1 | module_spec | spinelc: /Users/ryanseys/dev/spec/language/module_spec.rb: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-80fe6faf | 1 | execution_spec | spinelc: /Users/ryanseys/dev/spec/language/execution_spec.rb: unsupported syntax at "`echo disc #{ip}`" (spike handles only what the 7 example programs need) |
| ? | auto-ba2b7615 | 1 | precedence_spec | spinelc: /Users/ryanseys/dev/spec/language/precedence_spec.rb: unsupported syntax at "from..to" (spike handles only what the 7 example programs need) |
| ? | auto-bcd16e8b | 1 | assignments_spec | spinelc: /Users/ryanseys/dev/spec/language/assignments_spec.rb: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-bf7964ee | 1 | undef_spec | spinelc: /Users/ryanseys/dev/spec/language/undef_spec.rb: unsupported syntax at "undef meth" (spike handles only what the 7 example programs need) |
| ? | auto-e0dfb590 | 1 | defined_spec | spinelc: /Users/ryanseys/dev/spec/language/defined_spec.rb: /Users/ryanseys/dev/spec/language/fixtures/defined.rb: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-f0c6b32f | 1 | END_spec | uncaught exception: it_behaves_like |
| c | compound-assign | 1 | optional_assignments_spec | spinelc: /Users/ryanseys/dev/spec/language/optional_assignments_spec.rb: `[]`-style compound assignment only supports a single index argument (spike scope) |
| k | pattern-shapes | 1 | pattern_matching_spec | spinelc: /Users/ryanseys/dev/spec/language/pattern_matching_spec.rb: a pattern can't bind a variable inside a `\|` alternation (spike scope, matches real Ruby) |
| h | range-shapes | 1 | range_spec | ERROR: Literal Ranges creates beginless ranges: NoMethodError: undefined method 'new' for class Range |
| g | super-arity | 1 | super_spec | spinelc: /Users/ryanseys/dev/spec/language/super_spec.rb: /Users/ryanseys/dev/spec/language/fixtures/super.rb: unsupported statement in `class << self` (spike scope) -- only `def`s, constants, `include`, and `attr_*`/`private`/`alias` are handled here; `extend`/`prepend`/ivars/a nested `class << self` aren't supported yet |

List one bucket's tests: `cargo run -p xtask -- conformance triage --bucket <name>`.
