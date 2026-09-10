# `reduce(:>>)` and `reduce(:<<)` over an array of lambdas, beside `reduce(:+)`.
fns = [->(x) { x + 1 }, ->(x) { x * 2 }]
combined = fns.reduce(:>>)
p combined.call(3)
back = fns.reduce(:<<)
p back.call(3)
p [1, 2, 3].reduce(:+)
p [1, 2, 3].reduce(:*)
__END__
8
7
6
6
