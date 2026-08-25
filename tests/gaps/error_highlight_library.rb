# `error_highlight` is now VENDORED (`gems/error_highlight`, v0.7.2, byte-
# identical to the oracle's own copy), and the three lines below pass. What
# does not yet work is the thing the library is FOR: `detailed_message`
# returns no code snippet.
#
# Two of its three blockers went 2026-08-24 and have goldens of their own:
#
#   the primitive        `node_id_for_backtrace_location` answers -- see
#                        `tests/ast_node_id_for_backtrace_location.rb`.
#   the refusal          `AST.of` now raises the "compiled by prism"
#                        RuntimeError for a Method and a Location, which is
#                        what the gem RESCUES to reach its prism path. It
#                        answered nil, so the rescue never fired -- see
#                        `tests/ast_of_refuses_what_ruby_refuses.rb`.
#   the branch           `Exception.method_defined?(:detailed_message)`
#                        folded FALSE, so the gem installed its pre-3.2
#                        `to_s` branch -- see
#                        `tests/method_defined_on_a_bootstrap_class.rb`.
#
# WHAT IS LEFT is one line of the gem: `prism_find` opens with
# `require "prism"`, and that require sits inside a METHOD BODY.
#
# A deferred literal `require` of an in-tree `ext/` feature is folded to a
# `FeatureLoaded` marker plus `true` -- "a builtin needs no splice". For
# `prism` that is false. Its ext rows are ALL PRIVATE by design (they are the
# native half of the vendored gem, `ext/prism.rs` says so), so the marker
# activates an EMPTY module: `Prism.constants` is 0 and `Prism::VERSION`
# raises. A TOP-LEVEL `require "prism"` is fine, because the loader splices
# the gem before lowering ever sees the call. The same split exists for every
# feature that is both a gated ext and a vendored gem -- 14 of them today.
#
# Making the deferred path load the gem is NOT a one-line change: probed
# 2026-08-24, the unit then registers prism's node classes a second time and
# `node.rb` raises `superclass mismatch for class ClassVariableAndWriteNode`.
# That is the real work, and it is a loader question rather than an
# error_highlight one.
#
# THE FIRST LINE IS NOT WHAT IT LOOKS LIKE. A plain `ruby` answers `false` to
# the require, because error_highlight is loaded by default. The GOLDEN
# HARNESS runs the oracle with `--disable-error_highlight`, so the library is
# NOT loaded there and the require answers `true`. A golden's oracle is the
# harness's invocation, not the one you type.

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
