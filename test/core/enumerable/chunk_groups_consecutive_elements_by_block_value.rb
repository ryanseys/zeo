# Range#chunk keyed on even?, on a modulus, and on a comparison, the last
# summing each group.
p (1..8).chunk { |x| x.even? }.to_a
p (1..5).chunk { |x| x % 3 }.to_a
p (1..6).chunk { |x| x < 4 }.map { |k, xs| [k, xs.sum] }
__END__
[[false, [1]], [true, [2]], [false, [3]], [true, [4]], [false, [5]], [true, [6]], [false, [7]], [true, [8]]]
[[1, [1]], [2, [2]], [0, [3]], [1, [4]], [2, [5]]]
[[true, 6], [false, 15]]
