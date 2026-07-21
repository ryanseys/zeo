# Bundled tests:
#   - multi_assign_ivar
#   - multi_write_const

# === multi_assign_ivar ===
class T_multi_assign_ivar_A
  attr_reader :x
  def initialize; @x = 11; end
end
class T_multi_assign_ivar_B
  attr_reader :y
  def initialize; @y = 22; end
end
class T_multi_assign_ivar_C
  attr_reader :z
  def initialize; @z = 33; end
end

class T_multi_assign_ivar_Driver
  def self.load
    a = T_multi_assign_ivar_A.new
    b = T_multi_assign_ivar_B.new
    c = T_multi_assign_ivar_C.new
    return a, b, c
  end
end

class T_multi_assign_ivar_Holder
  def initialize
    @a, @b, @c = T_multi_assign_ivar_Driver.load
  end
  def show
    puts "a.x=#{@a.x} b.y=#{@b.y} c.z=#{@c.z}"
  end
end

h = T_multi_assign_ivar_Holder.new
h.show

# === multi_write_const ===
# Multi-write to constants — `A, B, T_multi_write_const_C = expr` — used to silently
# drop the targets because the parser emitted ConstantTargetNode as
# `UnknownNode_43` and the codegen had no MultiWriteNode branch in
# the constant-collection passes. Subsequent reads of A/B/T_multi_write_const_C compiled
# to undefined-identifier errors.
#
# Two RHS shapes are exercised: a literal ArrayNode (split element
# by element at emit time) and a call returning int_array (evaluated
# once into a temp, then sp_IntArray_get for each target).

class T_multi_write_const_C
  N1, N2, N3 = [10, 20, 30]
end

puts T_multi_write_const_C::N1
puts T_multi_write_const_C::N2
puts T_multi_write_const_C::N3

class T_multi_write_const_D
  ARR = [100, 200, 300, 400]
  M1, M2, M3, M4 = ARR
end

puts T_multi_write_const_D::M1
puts T_multi_write_const_D::M2
puts T_multi_write_const_D::M3
puts T_multi_write_const_D::M4

# RHS with a block — the block param introduces a local that
# must be declared in main() (or the enclosing scope) so the
# emitted `lv_<bp>` reference resolves. Single-const init already
# scans @const_expr_ids; the multi-write form needs the same scan
# over @multi_const_inits.
P, Q = [1, 2].map { |n| n * 10 }
puts P
puts Q

