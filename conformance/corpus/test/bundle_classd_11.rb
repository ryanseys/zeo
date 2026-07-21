# Bundled tests:
#   - multi_write_from_poly_recv
#   - multi_write_ivar
#   - multi_write_ivar_widening

# === multi_write_from_poly_recv ===
# Multi-write `a, b = rhs` where rhs's static type is `poly` (an
# sp_RbVal that carries a poly_array at runtime).  Pre-fix:
# `multi_write_target_type` fell through to the default `int` for
# rt == "poly", so `a` / `b` were declared as mrb_int and the
# emit-side dispatch (which DOES handle `val_t_local == "poly"` by
# unboxing to sp_PolyArray*) wrote `(sp_PolyArray_get(...)).v.i`,
# truncating the poly elements to garbage ints.

class T_multi_write_from_poly_recv_C
  def setup
    @h = {}
    [0, 1].each do |bank|
      [0, 1].each do |idx|
        # Outermost ||= produces a poly_poly_hash slot, then a
        # poly_array of poly_arrays of two-tuples — same shape as
        # optcarrot's @lut_update.
        (((@h[bank] ||= [])[idx] ||= [nil, nil])[0] ||= []) << [idx * 10 + bank, idx + bank]
      end
    end
  end

  def name_lut_size(bank, idx)
    # Multi-write whose RHS infers to `poly`: @h[bank][idx] indexes
    # into poly_poly_hash (returns poly), then into the unboxed
    # poly_array (returns poly).
    name_lut_update, _attr_lut_update = @h[bank][idx]
    name_lut_update.length
  end
end

c = T_multi_write_from_poly_recv_C.new
c.setup
# bank=1 / idx=0 yields a 1-element list (one push at this slot).
puts c.name_lut_size(1, 0)

# === multi_write_ivar ===
# `@a, @b = expr1, expr2` (multi-write to ivars) was not picked up by
# `scan_ivars`, so the ivars were never registered and the struct
# came out missing them. The emit-time path in compile_stmt already
# handled InstanceVariableTargetNode; the gap was only in the
# collection pass.

class T_multi_write_ivar_Inner
  def initialize(x); @x = x; end
  attr_reader :x
end

class T_multi_write_ivar_HasObjects
  def initialize
    @left, @right = T_multi_write_ivar_Inner.new(7), T_multi_write_ivar_Inner.new(8)
  end
  def sum
    @left.x + @right.x
  end
end

class T_multi_write_ivar_Holder
  def initialize
    @a, @b = 1, 2
  end
  attr_reader :a, :b
  def has_obj
    T_multi_write_ivar_HasObjects.new.sum
  end
end

h = T_multi_write_ivar_Holder.new
puts h.a
puts h.b
puts h.has_obj

# === multi_write_ivar_widening ===
class T_multi_write_ivar_widening_Pair
  def initialize(a, b)
    @x, @y = [a, b]
  end
  attr_reader :x, :y
end

p = T_multi_write_ivar_widening_Pair.new("hello", "world")
puts p.x
puts p.y

