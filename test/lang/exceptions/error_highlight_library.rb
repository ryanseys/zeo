# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# ErrorHighlight::Spotter holds an `@fetch` lambda that reads `@node` through self, so the object and its proc own each other. The other seven are the prism node that ring keeps alive.
#@ gccheck: cycle leak: 9 objects (Prism::Location x3, Array x1, ErrorHighlight::Spotter x1, Prism::ASCIISource x1, Prism::CallNode x1, Prism::NilNode x1, Proc x1)
# `error_highlight` works end to end: `detailed_message` answers the code
# snippet with the caret run under the offending call, which is the thing
# the library is FOR.
#
# Four blockers had to go, and each has a golden of its own:
#
#   the primitive        `node_id_for_backtrace_location` answers -- see
#                        `test/core/tracepoint/ast_node_id_for_backtrace_location.rb`.
#   the refusal          `AST.of` raises the "compiled by prism"
#                        RuntimeError for a Method and a Location, which is
#                        what the gem RESCUES to reach its prism path -- see
#                        `test/lang/exceptions/ast_of_refuses_what_ruby_refuses.rb`.
#   the branch           `Exception.method_defined?(:detailed_message)`
#                        folded FALSE, so the gem installed its pre-3.2
#                        `to_s` branch -- see
#                        `test/core/process/method_defined_on_a_bootstrap_class.rb`.
#   the require          `prism_find` opens with `require "prism"` inside a
#                        METHOD BODY, and prism is BOTH a gated ext and a
#                        vendored gem. The deferred require folded to "a
#                        builtin needs no splice" and activated an empty
#                        module -- see
#                        `test/core/kernel/a_runtime_required_gems_dependency_loads.rb`.
#
# THE FIRST LINE IS NOT WHAT IT LOOKS LIKE. A plain `ruby` answers `false`
# to the require, because error_highlight is loaded by default. The GOLDEN
# HARNESS runs ruby with `--disable-error_highlight`, so the library is NOT
# loaded there and the require answers `true`. A golden's reference output
# is the harness's invocation, not the one you type.

p require("error_highlight")
p defined?(ErrorHighlight)
p ErrorHighlight.respond_to?(:spot)

# The thing the library is FOR. `spot` needs `prism_find`, which needs a
# deferred `require "prism"`.
def boom
  nil.nope
rescue NoMethodError => e
  puts e.detailed_message(highlight: false)
end
boom
__END__
true
"constant"
true
undefined method 'nope' for nil (NoMethodError)

  nil.nope
     ^^^^^
