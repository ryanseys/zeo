g = [[1, 2], [3, 4]]
v = g.reduce([0]) { |a, r| a + r }
p v

g = [[1, 2], [3, 4]]
v = g.inject([0]) { |a, r| a + r }; p v   # Ruby: [0, 1, 2, 3, 4]   Spinel: C compile abort
p g.inject([]) { |a, r| a + r }            # => [1, 2, 3, 4]

g = [[1, 2], [3, 4]]
p(g.reduce([9]) { |a, r| a + r })
p(g.reduce(["s"]) { |a, r| a + r })
p([1, 2].reduce([0]) { |a, x| a << x })
p([[1], [2]].inject([5]) { |a, r| a | r })
