# deconstruct answers the member values, destructured into two locals, for a
# value read out of an array.
# (spinel issue #2940)
V = Data.define(:x, :y)
first = [V.new(1, 2)].first
a, b = first.deconstruct
p [a, b]
p first.deconstruct
S = Struct.new(:m, :n)
s = [S.new(5, 6), S.new(7, 8)].last
c, d = s.deconstruct
p [c, d]
__END__
[1, 2]
[1, 2]
[7, 8]
