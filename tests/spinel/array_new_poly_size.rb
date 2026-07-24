# spinel-dev#24: Array.new(size, fill) where `size` is a poly expression.
#
# When a factor of the size widens to poly (here Mat#initialize's `ncols`, bound
# poly at the `Mat.new(1, @d)` call site where `@d` is an untyped ivar), the
# size `nrows * ncols` emits `sp_poly_mul(...)` which returns sp_RbVal — but the
# Array.new lowering declared the size temp `mrb_int` and assigned the sp_RbVal
# straight into it ("incompatible types when initializing 'mrb_int' using
# 'sp_RbVal'"). The element type (float, from 0.0) was inferred correctly; only
# the size operand was uncoerced. Fix: emit the size via emit_int_expr
# (sp_poly_to_i for a poly value), like other array-bound sites.
class Mat
  def initialize(nrows, ncols)
    @flat = Array.new(nrows * ncols, 0.0)
  end
  def len
    @flat.length
  end
end

# Uncalled, but param inference scans its call site: Mat.new(1, @d) with @d an
# untyped (poly) ivar widens Mat#initialize's ncols to poly. Combined with the
# concrete Mat.new(2, 3) below, that's the heterogeneous-arg shape that poisoned
# the size operand.
class Other
  def go
    Mat.new(1, @d)
  end
end

m = Mat.new(2, 3)
puts m.len
