# Two Ranges with the same bounds are one Hash key. A fresh box hashed by
# pointer, so a lookup never found the entry it had just stored.
h = { (1..3) => :a, (4..6) => :b }
p h[(1..3)]
p h[(4..6)]
p h[(7..9)]
p h.key?(1..3)
p h.key?(1...3)
k = (1..3)
p h[k]
p h.size

g = {}
g[(1...4)] = :x
p g[(1...4)]
p g[(1..4)]
p g.size

# the Range value comparisons themselves are unchanged
p((1..3) == (1..3))
p((1..3).eql?(1..3))
p((1..3).hash == (1..3).hash)
