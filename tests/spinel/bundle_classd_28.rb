# Bundled tests:
#   - poly_return_to_str_unbox
#   - poly_shl_array_push_dispatch

# === poly_return_to_str_unbox ===
# Issue #655: A function whose RBS declares `-> String` but
# whose body returns a poly (sp_RbVal) ternary tripped the
# `incompatible types when returning sp_RbVal from a function
# with incompatible result type 'const char *'` T_poly_return_to_str_unbox_C error. Canonical
# shape: `@hash[key] || ""` where the hash inferred to value-type
# Int (no writes observed) and the `||` ternary boxed both arms.
#
# Fix: compile_body_return_inner now unboxes via `.v.s` when
# expr_type is poly and return_type is string / mutable_str.
#
# This test pins runtime behaviour on the writes-observed case
# (which produces the real value) since the no-writes path's
# semantics are inherently lossy — Ruby's `0 || ""` returns 0
# (int is truthy), so a no-write hash returning the default-0
# Int and then taking `.v.s` reads the union as a NULL pointer
# (which `sp_str_length` guards as length 0). The standalone
# repro from the issue is included to verify T_poly_return_to_str_unbox_C compile.

class T_poly_return_to_str_unbox_C
  attr_reader :h
  def initialize
    @h = {}
    @h["a"] = "alpha"
    @h["b"] = "beta"
  end

  def get(key)
    @h[key] || ""
  end
end

c = T_poly_return_to_str_unbox_C.new
puts c.get("a")
puts c.get("b")
puts c.get("missing")
puts c.get("a").length
puts c.get("missing").length

# === poly_shl_array_push_dispatch ===
# `<<` on a poly recv used to always lower to sp_poly_shl, which
# was Integer-bit-shift only. An IntArray boxed into a poly slot
# (e.g. an ivar that the type-inference passes widened to poly
# because it received both nil and an array) had its `<<` push
# silently turn into a bit-shift of the encoded pointer/length,
# dropping the rhs.
#
# After the fix, sp_poly_shl dispatches by recv cls_id and invokes
# the matching Array#<< push. Falls through to bit-shift only when
# the recv is a non-array (genuine Integer<<int).

class T_poly_shl_array_push_dispatch_C
  def initialize
    # `nil` then concrete-array writes widen @v to poly (Issue #130
    # "definite int/nil + obj → poly"). Once poly, the runtime
    # carries cls_id INT_ARRAY at access time.
    @v = nil
    @v = [10, 20, 30]
  end
  def push(x); @v << x; end
  def length; @v.length; end
  def get(i); @v[i]; end
end

c = T_poly_shl_array_push_dispatch_C.new
c.push(99)
c.push(88)
puts c.length         # 5
puts c.get(0)         # 10
puts c.get(3)         # 99
puts c.get(4)         # 88

# Bit-shift on genuine Integer recv still works (the fall-through).
x = 1
puts x << 3           # 8

