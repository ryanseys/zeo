# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# A Struct holding itself, which is what the golden inspects.
#@ gccheck: cycle leak: 1 objects (S x1)
S = Struct.new(:x)
s = S.new(nil)
s.x = s
p s
__END__
#<struct S x=#<struct S:...>>
