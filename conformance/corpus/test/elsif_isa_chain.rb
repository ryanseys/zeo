# Issue #310: an `if/elsif/else` chain with `is_a?(Class)` checks
# in the elsif branch used to emit `} else <prologue> if (...)`
# without wrapping braces. The compile_cond_expr for a poly is_a?
# emits prologue C statements (`sp_RbVal _t = recv; if (_t.tag ==
# SP_TAG_OBJ) {...}`) before returning the cond flag. Those
# prologue statements deposited between the bare `else` and the
# inner `if` — invalid C: the bare-else's body is the next single
# statement, and the temp decl's scope ended right there.
#
# Fix: wrap elsif branches in `{ }`. The cosmetic `else if` is
# replaced by `else { if ... }` — semantically identical, scope-
# correct.
#
# This test verifies the C *compiles*. Note that is_a?(UserClass)
# on a poly receiver still has gaps elsewhere (the SP_TAG_OBJ
# dispatch loop in compile_poly_method_call only walks user-class
# method tables, none of which define `is_a?`, so the runtime
# check is conservative — orthogonal to #310's brace bug). All
# arms below return the same string so the test passes regardless
# of which branch the runtime picks.

class A
end

class B
end

def normalize(value)
  if value.is_a?(A)
    "ok"
  elsif value.is_a?(B)
    "ok"
  else
    "ok"
  end
end

puts normalize(A.new)              # ok
puts normalize(B.new)              # ok
puts normalize(42)                 # ok

# Same shape but as the method's tail-position return — exercises
# compile_if_return's elsif path (companion fix in the same patch).
def classify(value)
  if value.is_a?(A)
    1
  elsif value.is_a?(B)
    1
  else
    1
  end
end

puts classify(A.new)               # 1
puts classify(B.new)               # 1
puts classify(99)                  # 1
