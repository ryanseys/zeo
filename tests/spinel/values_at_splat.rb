# values_at with a splatted array expands each element as a separate
# key/index (#3277).
a = [10, 20, 30, 40, 50]
idx = [1, 3]
p a.values_at(*idx)
p a.values_at(0, *idx, 4)
p a.values_at(*[])

h = { "a" => 1, "b" => 2, "c" => 3 }
keys = ["a", "c"]
p h.values_at(*keys)
p h.values_at("a", *["b", "c"])
p h.values_at(*["x", "a"])

sh = { one: 10, two: 20 }
p sh.values_at(*[:two, :one])

pa = [1, "two", :three, 4.0]
p pa.values_at(*[1, 3])
