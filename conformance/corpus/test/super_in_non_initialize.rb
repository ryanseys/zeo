# Issue #401: `super` in a non-`initialize` instance method
# previously emitted literal `0` instead of a call to the parent
# method. Compiled cleanly when the method's return type was
# `mrb_int` (silent wrong-result), failed C-compile when types
# diverged.
#
# Fix: SuperNode / ForwardingSuperNode in compile_expr now resolve
# the parent's same-named method (find_method_owner walks the
# parent chain) and emit a direct C call with `self` cast to the
# owner's pointer type. infer_type also returns the parent's
# method's return type for the result.

class A
  def value
    10
  end

  def doubled(x)
    x * 2
  end
end

class B < A
  def value
    super         # bare super forwards no args
  end

  def doubled(x)
    super(x) + 1  # explicit super(arg) -> A.doubled(x) + 1
  end
end

class C < B
  def value
    super + 5     # B.value calls A.value (10) -> 10 + 5 = 15
  end
end

b = B.new
puts b.value.to_s       # 10
puts b.doubled(3).to_s  # 6 + 1 = 7

c = C.new
puts c.value.to_s       # 15
