# Issue #395 string-yield sub-variant. The previous fix only
# covered int-yielding `each` over a user class; a string-yielding
# variant still tripped a type mismatch (block param `k` got
# declared `mrb_int` at the parent scope, but the yield expansion
# assigned a `const char *` from the iterating method's body).
#
# Fix: scan_locals' obj-recv branch now scans the yielding method's
# body in a temp scope (with @current_class_idx pointing at the
# method's owner class so ivar reads resolve), then asks
# body_yield_arg_types for the per-position yield arg types and
# pushes those onto the block-param `types` array.

class C
  def initialize
    @keys = []
    @keys << "alpha"
    @keys << "beta"
  end

  def each
    i = 0
    while i < @keys.length
      k = @keys[i]
      yield k
      i += 1
    end
    self
  end
end

c = C.new
c.each { |k| puts k }
