# GAP -- imported from the spinel corpus at c55d9bdb.
# Struct#dig with an unknown member raises NameError where ruby answers nil.
#
S = Struct.new(:a, :b)
s = S.new(1, 2)
p s.dig(:zz)
p s.dig(5)
p s.dig("a")
p s.dig(:a)
k = :a
p s.dig(k)
i = 1
s[i] = 9
p s.b
v = (s[i] = 7)
p v
k2 = :a
w = (s[k2] = 8)
p w
p s.values_at
__END__
nil
nil
1
1
1
9
7
8
[]
