# Expression-form hash `[]=`: Ruby's `(h[k] = v)` mutates the hash and
# evaluates to the assigned value. Covers str / sym keys and int / string
# values, including a poly-valued hash where the result keeps its scalar type.

h = {}
x = (h["a"] = 1)
puts x
puts h["a"]

hp = {"a" => 1, "b" => "x"}
y = (hp["c"] = 2)
puts y
puts hp["c"]
z = (hp["d"] = "added")
puts z
puts hp["d"]

hs = {a: 1}
w = (hs[:b] = 2)
puts w
puts hs[:b]
