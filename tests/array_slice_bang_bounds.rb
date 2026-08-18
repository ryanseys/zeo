# GAP -- imported from the spinel corpus at c55d9bdb.
# Array#slice! past the end answers [] where ruby answers nil.
#
a = [1, 2, 3]
p a.slice!(1, -1)
p a
b = [1, 2, 3]
p b.slice!(..1)
p b
c = [1, 2, 3]
p c.slice!(9, 2)
d = [1, 2, 3]
p d.slice!(0..1)
p d
e = [1, 2, 3]
p e.slice!(1..)
p e
f = ["x", "y", "z"]
p f.slice!(..0)
p f
g = [1, 2, 3]
p g.slice!(0, -2)
p g
