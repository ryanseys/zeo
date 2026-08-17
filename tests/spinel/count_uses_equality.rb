# Enumerable#count(item) counts elements EQUAL to its argument; the predicates
# take a pattern and match with ===. A class argument told them apart (#3817).
h = { a: 1, b: 2 }
p h.count(Array)
p h.count([:a, 1])
p h.any?(Array)
p h.none?(Array)
p h.one?(Array)
xs = [[1], [2], 3]
p xs.count(Array)
p xs.count([1])
p xs.any?(Array)
p [1, 2, 2].count(2)
p [1, 2, 2].count(Integer)
