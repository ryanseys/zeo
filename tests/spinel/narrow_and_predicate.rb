# Step 3 narrow extension. `if v.is_a?(C) && other_cond` should
# narrow `v` to C inside the then-arm just like the bare
# `if v.is_a?(C)` form. parse_is_a_predicate previously only
# matched a CallNode directly; an AndNode wrapping was rejected
# and the narrow was lost. Without the narrow, a call-site inside
# the then-arm widens the callee param with the unnarrowed type
# (typically poly) instead of the asserted concrete type.

def consume(s)
  s.length
end

class C
  attr_accessor :n
  def initialize; @n = 1; end
end

arr = ["hello", C.new]
v = arr[0]
allow = true
if v.is_a?(String) && allow
  puts consume(v)
end
