# `RubyVM::AbstractSyntaxTree.node_id_for_backtrace_location(loc)` -- the one
# primitive `error_highlight` asks of a prism-compiled program.
#
# THE RULE, oracle-verified: a location's node is the CALL at (path, lineno)
# whose NAME is the method that location INVOKED. That name is not on the
# Location and CRuby's has no row for it either, so it rides a private field
# `BacktraceLocation::callee` that reflection cannot see. Only the whole list
# can fill it: location i's callee is the method named by location i-1's
# label -- the frame it called into.
#
# Location 0 has no i-1, so its callee comes from what RAISED: a NameError
# names it, and an explicit `raise` is marked at the raise site (the frame's
# own label names the method the raise is IN, which has no call on that
# line). A cfunc frame's label IS the callee.
#
# When none of that answers, the primitive raises ArgumentError rather than
# guessing at a line with several calls on it. `ErrorHighlight.spot` already
# rescues exactly that into "no spot".
#
# The id is prism's own `pm_node_t.node_id`, assigned at ALLOCATION during
# the parse -- so it is neither pre- nor post-order over the finished tree and
# cannot be recomputed by walking. zeo's `RubyVM::AbstractSyntaxTree`
# numbering is a different scheme and cannot serve.

def id_for(loc)
  RubyVM::AbstractSyntaxTree.node_id_for_backtrace_location(loc)
rescue ArgumentError
  :no_spot
end

# A missing method: location 0's callee is `NameError#name`.
begin
  nil.nope
rescue NoMethodError => e
  p id_for(e.backtrace_locations.first)
end

# A chain through two Ruby frames, plus an explicit `raise` at the bottom.
def outer = inner
def inner = raise("x")
begin
  outer
rescue => e
  p e.backtrace_locations.map { |l| [l.lineno, id_for(l)] }
end

# A cfunc frame: its own label is the callee, and its caller names the same
# call.
begin
  [1, 2].fetch(9)
rescue IndexError => e
  p e.backtrace_locations.first(2).map { |l| [l.lineno, id_for(l)] }
end

# Two calls on ONE line pick different nodes, by name.
def boom(x) = x.nope
begin
  boom([1, 2].first)
rescue NoMethodError => e
  p id_for(e.backtrace_locations.first)
end

# `caller_locations` builds its own list, so it carries callees too.
def where = caller_locations(1, 1).first
p id_for(where)

# A non-Location argument is a TypeError, named.
begin
  RubyVM::AbstractSyntaxTree.node_id_for_backtrace_location(RubyVM::AbstractSyntaxTree)
rescue TypeError => e
  p [:type_error, e.class]
end
