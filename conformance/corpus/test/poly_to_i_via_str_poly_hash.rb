# `compile_poly_method_call` lacked a `to_i` arm: `(poly).to_i` fell
# through to `emit_poly_dispatch` → `emit_poly_builtin_dispatch`,
# which only handles `[]` / `length` / `size`. The dispatched
# result temp stayed at its `0` default, so any `.to_i` on a poly-
# typed receiver returned 0 instead of unboxing the int payload.
#
# Repro: heterogeneous Hash returns a poly value; `.to_i` on that.

class C
  def initialize
    @h = { "a" => 42, "b" => "hello" }
  end
  def at(k)
    (@h[k]).to_i
  end
end

c = C.new
puts c.at("a")
