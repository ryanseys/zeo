hs = [{ b: 2 }]
h = { a: 1 }
h.merge!(*hs)
p h
hs2 = [{ b: 2 }, { c: 3 }]
h2 = { a: 1 }
h2.update(*hs2)
p h2
g = { "a" => 1 }
gs = [{ "b" => 2 }]
g.merge!(*gs)
p g
k = { a: 1 }
k.merge!({ b: 2 })
p k
ks = [:a]
p({ a: 1, b: 2 }.except(*ks))
