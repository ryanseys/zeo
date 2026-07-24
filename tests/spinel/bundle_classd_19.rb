# Bundled tests:
#   - poly_hash_literal_from_ivar
#   - poly_int_arith_auto_unify
#   - poly_int_bit_index

# === poly_hash_literal_from_ivar ===
# Issue #287: a method that returns `{ key: @x, ... }` had its
# inferred return type frozen at the first scan, before #247 widened
# the ivar slot to poly via a sibling-writer disagreement. The hash
# literal then emitted as `sp_StrIntHash *` (int-valued) and tried
# to insert the now-poly ivar value at T_poly_hash_literal_from_ivar_C compile time.
#
# Fix: extend `infer_hash_val_type`'s all_same branch to handle
# `first_vt == "poly"` — sym/str-keyed poly hashes get the same
# *_poly_hash storage as the mixed-types `else` branch.

class T_poly_hash_literal_from_ivar_C
  def initialize
    @x = 0
  end
  def write_str(s)
    @x = s
  end
  def hash_of_x
    { key: @x }
  end
end

c = T_poly_hash_literal_from_ivar_C.new
c.write_str("hello")
puts c.hash_of_x.size       # 1
puts c.hash_of_x.has_key?(:key)  # true

# Multiple sym-keyed entries, all flowing from poly ivars.
class T_poly_hash_literal_from_ivar_D
  def initialize
    @a = 0
    @b = 0
  end
  def write(sa, sb)
    @a = sa
    @b = sb
  end
  def attrs
    { a: @a, b: @b }
  end
end

d = T_poly_hash_literal_from_ivar_D.new
d.write("alpha", "beta")
puts d.attrs.size           # 2
puts d.attrs.has_key?(:a)   # true
puts d.attrs.has_key?(:c)   # false

# === poly_int_arith_auto_unify ===
# Auto-unify int receiver + poly argument in arithmetic. Common
# case: `addr + arr[i][j]` where the inner dispatch is a
# heterogeneous user-class+IntArray array — both arms genuinely
# return int at runtime, but spinel statically types `[]`
# dispatch as poly. Spinel previously emitted `(addr +
# <sp_RbVal>)` and the C compile failed (`invalid operands to
# binary + (have 'mrb_int' and 'sp_RbVal')`). The operator-site
# fall-through now unboxes the poly arg via .v.i, mirroring PR
# #347's LV-write semantics.

# Class that defines `[]` returning int — when stored alongside an
# IntArray in a poly_array, the outer `[]` dispatch returns poly.
class T_poly_int_arith_auto_unify_Lookup
  def [](i); i * 10 + 1; end
end

arr = [T_poly_int_arith_auto_unify_Lookup.new, [100, 200, 300]]

# arr[i] is poly (T_poly_int_arith_auto_unify_Lookup or IntArray), then `[j]` dispatches —
# every runtime arm returns int, but the static return type is
# poly. So `addr + arr[1][0]` is `int + poly` at codegen time.
addr = 7
puts(addr + arr[1][0])  # 7 + 100 = 107
puts(addr - arr[0][2])  # 7 - 21  = -14
puts(addr * arr[1][1])  # 7 * 200 = 1400
puts(arr[1][2] / 3)     # 300 / 3 = 100  (recv-poly + arg-int already worked via sp_poly_div, included for coverage)

# === poly_int_bit_index ===
# `Integer#[N]` (bit-extraction) where the receiver was typed
# poly via cascading inference. Used to fall through to the
# SP_TAG_OBJ array dispatch in `emit_poly_builtin_dispatch`'s
# `[]` branch, leaving the result at the nil/0 default — so
# `int[i]` returned 0 for every i, and downstream
# `pc[8] == tmp[8] ? a : b` style page-bit checks always took
# the same-page branch.

class T_poly_int_bit_index_Holder
  # Mixed-type ivar writes widen @x to poly. Reads return sp_RbVal
  # even when the runtime value is concretely int.
  def initialize
    @x = "init"
    @x = 0b10110100  # 180 — actual value at test time
  end

  attr_reader :x

  # Same-page check pattern: extract bit 8 from each poly-typed int.
  def same_page?(a, b)
    a[8] == b[8]
  end
end

h = T_poly_int_bit_index_Holder.new
v = h.x  # `v` is poly here

# Static index, dynamic value. Without the fix, every output is 0.
puts v[0]    # 0
puts v[1]    # 0
puts v[2]    # 1
puts v[3]    # 0
puts v[4]    # 1
puts v[5]    # 1
puts v[6]    # 0
puts v[7]    # 1

# Dynamic index. Same buggy fallthrough.
i = 0
while i < 4
  puts v[i]
  i += 1
end

# Page-bit check: `0x100 | x` flips bit 8 on/off so the comparison
# meaningfully differs depending on input. With the bug both values
# get bit-8 == 0 (default), the comparison is always true, and
# branch-cycle counting collapses.
puts h.same_page?(h.x, h.x | 0x100)   # false (bit 8 differs)
puts h.same_page?(h.x, h.x | 0x10)    # true (bit 8 same — both 0)

