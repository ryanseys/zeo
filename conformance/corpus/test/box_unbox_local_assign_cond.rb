# `infer_type(LocalVariableWriteNode)` returned the RHS's static
# type rather than the slot's. `compile_cond_expr` uses the
# expression type to pick between `sp_poly_truthy(...)` and a
# direct test. Without the slot-type report, an
# `if (sprite = arr[i])` expression where `sprite`'s slot is poly
# (widened by other writers) but the RHS is scalar fell through
# to a direct test on the boxed value, which gcc rejects.

class C
  def initialize
    @arr = [10, "twenty", :thirty]   # poly_array
  end
  def find(i)
    if (entry = @arr[i])             # entry-slot is poly (widened by all the writes below)
      entry
    end
  end
end

c = C.new
puts c.find(0).to_s
puts c.find(1).to_s
