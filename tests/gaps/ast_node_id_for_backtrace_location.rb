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
# WHERE THE ID COMES FROM. `node_id` is a `uint32_t` on prism's
# own `pm_node_t` (ast.h:1074), assigned at ALLOCATION during the parse -- so
# it is neither pre- nor post-order over the finished tree (a StatementsNode
# is allocated before the call it holds), and cannot be recomputed by walking.
# Three ways to reach it, none free:
#
#   * the `ruby-prism` safe bindings expose no accessor -- but the field IS
#     reachable, corrected 2026-08-21: `pointer` is private on each node
#     STRUCT (`CallNode`), and `Node` is an ENUM whose variant fields are
#     public wherever the enum is, so `Node::CallNode { pointer, .. }`
#     destructures and `(*pointer.cast::<pm_node_t>()).node_id` reads it
#     (`ruby-prism-sys` 1.9.0, `ast.h:1074`). One arm for the call case,
#     ~170 for a general helper. So the id is NOT the blocker;
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
#
# WHAT ACTUALLY BLOCKS THIS, measured 2026-08-21: the CALLEE, not the id. A
# `Location` does not carry one, and the rule needs it -- location i's node
# is the call named by location i-1's method, and when i-1 is a BLOCK frame
# the node is the `yield` that ran it rather than a call at all.
# `exc_backtrace_locations` builds the whole list at once and so CAN thread
# the callee through, but `caller_locations` and the `Thread`/`Fiber`
# builders each need their own answer, and location 0's comes from the
# exception (`NameError#name` for a missing method). That is the project.
#
# A safety valve belongs with it: when the callee cannot be determined,
# raise `ArgumentError`. `ErrorHighlight.spot` already rescues that into "no
# spot", so a shape the rule misses degrades to a plain message instead of
# breaking every `detailed_message`.
begin
  nil.nope
rescue NoMethodError => e
  loc = e.backtrace_locations.first
  p RubyVM::AbstractSyntaxTree.node_id_for_backtrace_location(loc)
end
