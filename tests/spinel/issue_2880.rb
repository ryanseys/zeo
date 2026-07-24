fns = [->(x) { x + 1 }, ->(x) { x * 2 }]
combined = fns.reduce(:>>)
p combined.call(3)
back = fns.reduce(:<<)
p back.call(3)
p [1, 2, 3].reduce(:+)
p [1, 2, 3].reduce(:*)
