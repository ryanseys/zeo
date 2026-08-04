# An Array read out of an Array(Array) or a Hash value is carried in a poly
# slot. It answered Array to #class but only part of Array's surface worked:
# a Range index fell through the poly [] as index 0 and returned element 0,
# #min / #max had no arm for a String array, and the count-taking reads
# (first(n), last(n), take, drop, rotate, values_at, sample(n)) raised
# NoMethodError. #3464.
rows = [["a", "b", "c"]]
r = rows[0]
p r.class
p r[0..1]
p r.slice(0..1)
p r.min
p r.max
p r.first(2)
p r.last(2)
p r.take(2)
p r.drop(1)
p r.rotate
p r.values_at(0, 2)
p r[1..2]
p r[0..-1]
p r[0...2]
p r[1..]
h = { "row" => ["x", "y", "z"] }
p h["row"][0..1]
p h["row"].min
p h["row"].max
floats = [[1.5, 2.5]]
p floats[0][0..1]
p floats[0].min
syms = [[:a, :b]]
p syms[0][0..1]
ints = [[3, 1, 2]]
p ints[0][0..1]
p ints[0].min
p ints[0].max
p ints[0].first(2)
p ints[0].last(2)
p ints[0].take(2)
p ints[0].drop(1)
p ints[0].rotate
p ints[0].rotate(2)
p ints[0].values_at(0, 2)
p ints[0].sample(3).sort
nested = [["a", "b", "c"]]
g = nested[0]
p g[1..2]
p g[0..-1]
p g[0...2]
p g[1..]
p g[..1]
p g[5..6]
p g[3..4]
p g.first(10)
p g.drop(10)
p g.last(10)
p g.values_at(0, -1, 9)
p g.slice(1..2)
a = ["a", "b", "c"]
p a[0..1]
p a.first(2)
p a.min
