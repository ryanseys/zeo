# `RubyVM::AbstractSyntaxTree.node_id_for_backtrace_location(loc)` -- the one
# primitive `error_highlight` needs from a prism-compiled program, and so the
# blocker under `error_highlight_library.rb` too. zeo raises the same
# "compiled by prism" RuntimeError `.of` does, which is what sends the gem
# down its `prism_find` path -- and that path then calls this.
#
# THE RULE IS EXACT AND COSTS NOTHING AT RUNTIME. Oracle-probed over ten
# shapes: a location's node is the CALL at (path, lineno) whose NAME is the
# method that location invoked, and that name is already in the backtrace --
#
#   location i>0    the method of location i-1 (`def outer = inner` reports
#                   the `inner` call; a cfunc location's own label serves,
#                   since its position IS the caller's -- `[1,2].fetch(9)`
#                   reports the `fetch` call from both locations)
#   location 0      whatever raised: `NameError#name` for a missing method
#                   (`nil.nope` -> the `nope` call), else the location's own
#                   label (`Integer("zz")` -> the `Integer` call)
#
# Verified against `Integer(nil.qqq)` (51, the INNER call), `[1,2].nope.first`
# and `[1,2].first.nope` (which pick different calls on one line), and a plain
# `raise` (the `raise` call itself). No per-frame offset, no wider `Frame`, no
# side table -- which is what the earlier design sketch assumed was needed.
#
# THE BLOCKER IS THE ID, not the rule. `node_id` is a `uint32_t` on prism's
# own `pm_node_t` (ast.h:1074), assigned at ALLOCATION during the parse -- so
# it is neither pre- nor post-order over the finished tree (a StatementsNode
# is allocated before the call it holds), and cannot be recomputed by walking.
# Three ways to reach it, none free:
#
#   * the `ruby-prism` safe bindings expose no accessor and keep the raw
#     `*mut pm_call_node_t` private, so reading the field means walking with
#     `ruby-prism-sys` and its per-kind child switch;
#   * zeo's own `RubyVM::AbstractSyntaxTree` numbering is a DIFFERENT scheme
#     (its translator maps prism onto CRuby's NODE_* set and answers 3 where
#     prism answers 4), so it cannot serve;
#   * zeo's VENDORED prism gem does produce CRuby-identical ids -- verified
#     over a whole file -- so the primitive is implementable in Ruby, the way
#     the gem's own `prism_find` does it. That needs the callee on the
#     Location, and CRuby's `Thread::Backtrace::Location` has no such method,
#     so a public row would be a census divergence.
#
# The third is the shape to take, once there is somewhere to put the callee
# that reflection does not see.
begin
  nil.nope
rescue NoMethodError => e
  loc = e.backtrace_locations.first
  p RubyVM::AbstractSyntaxTree.node_id_for_backtrace_location(loc)
end
