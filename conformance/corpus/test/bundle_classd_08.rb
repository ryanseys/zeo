# Bundled tests:
#   - method_name_collision_imeth

# === method_name_collision_imeth ===
# Issue #407 (imeth follow-up): user-defined instance methods
# named `hash`, `to_i`, `to_s`, `length`, `size`, `count` were
# pre-empted by the hardcoded name -> int / string arms in
# `infer_method_name_type`. The arm returned without consulting
# the user's class table, so even when the recv was a typed
# obj_<C>, the call was inferred as int (Object#hash etc.) and
# the C compile cast the user's actual `const char *` return
# through (long long).
#
# Fix: each hardcoded arm defers (returns "") when (a) the recv
# is plausibly an obj_<C> at runtime (LocalVariableReadNode /
# CallNode / InstanceVariableReadNode -- not a literal primitive
# node), and (b) at least one user class declares the same imeth.
# Falling through lands at infer_recv_method_type, whose
# cls_method_return path resolves to the user's inferred return
# type. Primitive recvs (literal IntegerNode / StringNode / ...)
# still hit the hardcode, so 5.hash / "x".hash etc. stay int.
#
# Coverage:
#   - `hash` shadowed -- the canonical bcrypt-shape from Ori's
#     report, but in instance form.
#   - `to_s` shadowed -- collides with Object#to_s (string).
#   - `to_i` shadowed -- collides with Integer#to_i.
#   - `length` shadowed -- collides with String#length.

class T_method_name_collision_imeth_Item
  attr_accessor :tag
  def initialize(t); @tag = t; end
  def hash;   "item-hash:"   + @tag; end
  def to_i;   "item-toi:"    + @tag; end
  def to_s;   "item-tos:"    + @tag; end
  def length; "item-length:" + @tag; end
end

i = T_method_name_collision_imeth_Item.new("a")
puts i.hash
puts i.to_i
puts i.to_s
puts i.length

