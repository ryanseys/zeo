# Compound assignment to a constant-path target (`M::X += 2`). Resolves
# to the qualified constant slot and mutates it at runtime.
module M
  X = 0
  Y = 10
  Z = 0
end
M::X += 2
p M::X       # 2
M::Y -= 3
p M::Y       # 7
M::Z ||= 5
p M::Z       # 0 (truthy)
M::Z &&= 8
p M::Z       # 8
