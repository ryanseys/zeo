# Direct-call form (no caller-class wrapper): a top-level method
# `apply` is the caller, target is a parameter. Verifies the callee's
# param widening fires when the call is inside a top-level method
# whose param type for `t` is itself widened from the caller-side
# type.

def apply(t, arr)
  t.set_data(arr)
  t
end

class Target
  def set_data(arr)
    @arr = arr
  end
  def length
    @arr.length
  end
end

t = Target.new
apply(t, [10, 20, 30, 40])
puts t.length
