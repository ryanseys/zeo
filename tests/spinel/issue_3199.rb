items = [[1], {a: 1}]
items.each(&:clear)
p items

mixed = [[1, 2], {a: 1}, [3]]
mixed.each(&:clear)
p mixed

h = {x: 1}
p [h].map(&:clear)
p h
