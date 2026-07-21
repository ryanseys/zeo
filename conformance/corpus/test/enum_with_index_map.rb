# Enumerator#with_index return value on a stored map/each enumerator (#2510)
e = [1, 2, 3].map
p(e.with_index { |x, i| x * 10 + i })   # map enumerator -> collected array
g = [1, 2, 3].each
p(g.with_index { |x, i| p [x, i] })     # each enumerator -> source receiver
h = %w[a b].collect
p(h.with_index(1) { |x, i| "#{x}#{i}" })
