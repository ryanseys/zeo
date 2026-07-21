l = ->(k: 5) { k }
p(l.call(k: 9))
p(l.call)
p(->(a, k:) { [a, k] }.call(1, k: 2))
