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
__END__
2
7
0
8
#@ stderr
lang/constants/const_path_compound_assign.rb:8: warning: already initialized constant M::X
lang/constants/const_path_compound_assign.rb:4: warning: previous definition of X was here
lang/constants/const_path_compound_assign.rb:10: warning: already initialized constant M::Y
lang/constants/const_path_compound_assign.rb:5: warning: previous definition of Y was here
lang/constants/const_path_compound_assign.rb:14: warning: already initialized constant M::Z
lang/constants/const_path_compound_assign.rb:6: warning: previous definition of Z was here
