# Indexed-slot compound writes — `a[i] OP= v`, `a[i] &&= v`,
# `a[i] ||= v`, `a[i], b[j] = ...`. Was four tests; merged here.
# The op_assign section's `class Vec` is unique; locals reused
# across the originals (`xs`, `counts`, etc.) get per-section
# prefixes so the merged script's type inference doesn't unify
# them or carry hash content across sections.

class Vec
  attr_accessor :data
  def initialize(n)
    @data = Array.new(n, 0.5)
  end
end

# === Operator-write (`a[i] OP= v`) ===
# Float / Int / hash receivers, both directly and through an
# object field. IndexOperatorWriteNode is distinct from a `.[]=`
# call; without the dedicated codegen the update is dropped.

vop1 = Vec.new(4)
vop1.data[0] += 1.5
vop1.data[1] -= 0.5
vop1.data[2] *= 4.5
vop1.data[3] /= 0.5
puts vop1.data[0]   # 2  (spinel strips ".0")
puts vop1.data[1]   # 0
puts vop1.data[2]   # 2.25
puts vop1.data[3]   # 1

ints_op = Array.new(3, 10)
ints_op[0] += 5
ints_op[1] -= 3
ints_op[2] *= 2
puts ints_op[0]     # 15
puts ints_op[1]     # 7
puts ints_op[2]     # 20

counts_op = {"a" => 1, "b" => 2}
counts_op["a"] += 10
counts_op["b"] += 20
puts counts_op["a"] # 11
puts counts_op["b"] # 22

# Same pattern through a method call on an object: `obj.attr[i] += x`.
vop2 = Vec.new(3)
iop = 0
while iop < 3
  vop2.data[iop] += 0.25
  iop += 1
end
puts vop2.data[0]   # 0.75
puts vop2.data[1]   # 0.75
puts vop2.data[2]   # 0.75

# === And-write (`a[i] &&= v`) ===
# Reads a[i]; if truthy, sets a[i] = v. Stick to non-zero values
# so the C-truthy semantic agrees with Ruby's truthy semantic.
xs_aw = [1, 2, 3, 4, 5]
xs_aw[0] &&= 10              # truthy -> fires
xs_aw[2] &&= 30              # truthy -> fires
xs_aw[4] &&= xs_aw[4] * 100  # truthy -> fires; recv/idx evaluated once
puts xs_aw[0]                # 10
puts xs_aw[1]                # 2
puts xs_aw[2]                # 30
puts xs_aw[3]                # 4
puts xs_aw[4]                # 500

counts_aw = {"alice" => 30, "bob" => 25}
counts_aw["alice"] &&= 31    # truthy -> fires
counts_aw["carol"] &&= 50    # unset (returns 0) -> doesn't fire
puts counts_aw["alice"]      # 31
puts counts_aw["bob"]        # 25
puts counts_aw.length        # 2 (carol not added)

# === Or-write (`a[i] ||= v`) ===
# Reads a[i]; if falsy, sets a[i] = v. Receiver and index are
# evaluated exactly once even though the source has them on both
# sides of the implicit `a[i] = a[i] || v`.
counts_or = {"alice" => 0, "bob" => 5}
counts_or["carol"] ||= 10    # never set -> fires
counts_or["bob"]   ||= 99    # 5 truthy -> no-op
puts counts_or["alice"]      # 0
puts counts_or["bob"]        # 5
puts counts_or["carol"]      # 10

# === Target (multi-assign LHS into indexed slots) ===
# `a[0], b[1] = 1, 2` -- each LHS slot is an IndexTargetNode.
xs_t = [10, 20, 30]
ys_t = [100, 200, 300]
xs_t[0], ys_t[2] = 1, 999
puts xs_t[0]                 # 1
puts xs_t[1]                 # 20
puts ys_t[2]                 # 999

# Same-array two-slot swap.
zs_t = [1, 2, 3, 4, 5]
zs_t[0], zs_t[4] = zs_t[4], zs_t[0]
puts zs_t[0]                 # 5
puts zs_t[4]                 # 1

# Hash slot mixed with array slot in one multi-assign.
counts_t = {"a" => 0, "b" => 0}
arr_t = [0, 0, 0]
counts_t["a"], arr_t[1] = 42, 99
puts counts_t["a"]           # 42
puts arr_t[1]                # 99
