# Constant-path multi-assignment targets (`a, M::X = 1, 2`). Each
# ConstantPathTargetNode resolves to the qualified constant's slot so
# the destructured value lands in the right place.
module M
  X = 0
  Y = 0
end
a, M::X = 1, 2
p [a, M::X]
b, M::Y = 10, 20
p [b, M::Y]
