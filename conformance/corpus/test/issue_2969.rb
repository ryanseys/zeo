h = {}
[1, 2, 3].each_with_object(h) { |x, acc| acc[x] = x * x }
p h
