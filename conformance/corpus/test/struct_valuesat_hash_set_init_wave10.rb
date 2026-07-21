# Struct#values_at (index and range keys) and #hash (value equality).
Pt = Struct.new(:x, :y, :z)
a = Pt.new(1, 2, 3)
p a.values_at(0, 2)
p a.values_at(0..1)
b = Pt.new(1, 2, 3)
p(a.hash == b.hash)
c = Pt.new(9, 2, 3)
p(a.hash == c.hash)
require 'set'
t = Set.new([1, 2, 2, 3])
p t
