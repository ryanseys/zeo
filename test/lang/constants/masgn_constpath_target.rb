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
__END__
[1, 2]
[10, 20]
#@ stderr
lang/constants/masgn_constpath_target.rb:8: warning: already initialized constant M::X
lang/constants/masgn_constpath_target.rb:5: warning: previous definition of X was here
lang/constants/masgn_constpath_target.rb:10: warning: already initialized constant M::Y
lang/constants/masgn_constpath_target.rb:6: warning: previous definition of Y was here
