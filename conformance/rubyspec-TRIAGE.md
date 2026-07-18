# Gap triage

Failing tests grouped by normalized failure message, ranked by how many
tests each gap blocks. Clusters refer to the implementation plan's gap
families. Oracle `ruby 4.0.5 (2026-05-20 revision 64336ffd0e) +PRISM [arm64-darwin25]`.

| cluster | bucket | blocked | sample test | sample message |
|---|---|---|---|---|
| ? | spike-misc | 33 | alias_spec | spinelc: /Users/ryanseys/dev/spec/language/alias_spec.rb: `class << obj` (a per-instance singleton class) supports only instance `def`s here (spike scope) |
| a | splat | 2 | for_spec | spinelc: /Users/ryanseys/dev/spec/language/for_spec.rb: expected `*name` as a multi-assignment's splat target |
| ? | auto-01a86947 | 1 | numbered_parameters_spec | spinelc: /Users/ryanseys/dev/spec/language/numbered_parameters_spec.rb: eval("..."): parse error: numbered parameter is already used in outer block |
| ? | auto-083ceea9 | 1 | precedence_spec | spinelc: /Users/ryanseys/dev/spec/language/precedence_spec.rb: eval("..."): parse error: unexpected '<=>'; '<=>' is a non-associative operator |
| ? | auto-1cd8ccca | 1 | it_parameter_spec | spinelc: /Users/ryanseys/dev/spec/language/it_parameter_spec.rb: eval("..."): eval("..."): parse error: 'it' is not allowed when an ordinary parameter is defined |
| ? | auto-21cbb640 | 1 | send_spec | spinelc: /Users/ryanseys/dev/spec/language/send_spec.rb: eval("..."): parse error: both block arg and actual block given; only one block is allowed |
| ? | auto-2564c2d5 | 1 | retry_spec | spinelc: /Users/ryanseys/dev/spec/language/retry_spec.rb: eval("..."): parse error: Invalid retry without rescue |
| ? | auto-411aa863 | 1 | heredoc_spec | spinelc: /Users/ryanseys/dev/spec/language/heredoc_spec.rb: eval("..."): parse error: unterminated here document identifier |
| ? | auto-4e7163de | 1 | break_spec | spinelc: /Users/ryanseys/dev/spec/language/break_spec.rb: eval("..."): parse error: Invalid break |
| ? | auto-51c70a78 | 1 | metaclass_spec | spinelc: /Users/ryanseys/dev/spec/language/metaclass_spec.rb: /Users/ryanseys/dev/spec/fixtures/class.rb: eval("..."): parse error: Invalid yield |
| ? | auto-5b5fe311 | 1 | class_spec | spinelc: /Users/ryanseys/dev/spec/language/class_spec.rb: /Users/ryanseys/dev/spec/fixtures/class.rb: eval("..."): parse error: Invalid yield |
| ? | auto-619a71ee | 1 | block_spec | spinelc: /Users/ryanseys/dev/spec/language/block_spec.rb: unsupported syntax at "\"a\" => 1, a: 10" (spike handles only what the 7 example programs need) |
| ? | auto-6a6cde2b | 1 | BEGIN_spec | spinelc: /Users/ryanseys/dev/spec/language/BEGIN_spec.rb: eval("..."): parse error: BEGIN is permitted only at toplevel |
| ? | auto-72ea658f | 1 | module_spec | spinelc: /Users/ryanseys/dev/spec/language/module_spec.rb: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-7446f89e | 1 | delegation_spec | spinelc: /Users/ryanseys/dev/spec/language/delegation_spec.rb: eval("..."): parse error: unexpected `*`; anonymous rest parameter is also used within block |
| ? | auto-76638974 | 1 | safe_navigator_spec | spinelc: /Users/ryanseys/dev/spec/language/safe_navigator_spec.rb: eval("..."): parse error: unexpected '{'; expecting a message to send to the receiver |
| ? | auto-77e4ce34 | 1 | numbers_spec | spinelc: /Users/ryanseys/dev/spec/language/numbers_spec.rb: eval("..."): parse error: unexpected '.', ignoring it |
| ? | auto-80fe6faf | 1 | execution_spec | spinelc: /Users/ryanseys/dev/spec/language/execution_spec.rb: unsupported syntax at "`echo disc #{ip}`" (spike handles only what the 7 example programs need) |
| ? | auto-9d5f0395 | 1 | def_spec | spinelc: /Users/ryanseys/dev/spec/language/def_spec.rb: eval("..."): parse error: unexpected parameter `*` |
| ? | auto-b326665a | 1 | symbol_spec | spinelc: /Users/ryanseys/dev/spec/language/symbol_spec.rb: eval("..."): parse error: invalid symbol |
| ? | auto-bcd16e8b | 1 | assignments_spec | spinelc: /Users/ryanseys/dev/spec/language/assignments_spec.rb: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-bd09d25d | 1 | case_spec | spinelc: /Users/ryanseys/dev/spec/language/case_spec.rb: eval("..."): parse error: expected a `when` or `in` clause after `case` |
| ? | auto-bf7964ee | 1 | undef_spec | spinelc: /Users/ryanseys/dev/spec/language/undef_spec.rb: unsupported syntax at "undef meth" (spike handles only what the 7 example programs need) |
| ? | auto-d6827073 | 1 | singleton_class_spec | spinelc: /Users/ryanseys/dev/spec/language/singleton_class_spec.rb: /Users/ryanseys/dev/spec/fixtures/class.rb: eval("..."): parse error: Invalid yield |
| ? | auto-de68229a | 1 | redo_spec | spinelc: /Users/ryanseys/dev/spec/language/redo_spec.rb: eval("..."): parse error: Invalid redo |
| ? | auto-e0dfb590 | 1 | defined_spec | spinelc: /Users/ryanseys/dev/spec/language/defined_spec.rb: /Users/ryanseys/dev/spec/language/fixtures/defined.rb: expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| ? | auto-f0c6b32f | 1 | END_spec | uncaught exception: it_behaves_like |
| c | compound-assign | 1 | optional_assignments_spec | spinelc: /Users/ryanseys/dev/spec/language/optional_assignments_spec.rb: `[]`-style compound assignment only supports a single index argument (spike scope) |
| k | pattern-shapes | 1 | pattern_matching_spec | spinelc: /Users/ryanseys/dev/spec/language/pattern_matching_spec.rb: `eval` with a non-literal argument isn't supported yet (spike scope) -- only a plain string literal, e.g. `eval("1 + 2")`, is currently accepted; dynamic `eval` needs a runtime parser/interpreter (see docs/EVAL_VM.md) |
| h | range-shapes | 1 | range_spec | ERROR: Literal Ranges creates beginless ranges: NoMethodError: undefined method 'new' for class Range |
| g | super-arity | 1 | super_spec | spinelc: /Users/ryanseys/dev/spec/language/super_spec.rb: /Users/ryanseys/dev/spec/language/fixtures/super.rb: unsupported statement in `class << self` (spike scope) -- only `def`s, constants, `include`, and `attr_*`/`private`/`alias` are handled here; `extend`/`prepend`/ivars/a nested `class << self` aren't supported yet |

List one bucket's tests: `cargo run -p xtask -- conformance triage --bucket <name>`.
