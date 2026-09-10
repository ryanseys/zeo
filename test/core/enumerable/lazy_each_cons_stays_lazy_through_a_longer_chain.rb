# each_cons followed by map and first, bound to a local, and re-lazied before a
# select.
r = (1..Float::INFINITY).lazy.each_cons(2).map { |a, b| a + b }.first(3)
p r
lz = (1..Float::INFINITY).lazy.each_cons(2)
p lz.first(3)
p((1..Float::INFINITY).lazy.each_cons(2).lazy.select { |a, b| b - a == 1 }.first(2))
p((1..8).lazy.select { |n| n.odd? }.each_cons(2).map { |a, b| a * b }.to_a)
__END__
[3, 5, 7]
[[1, 2], [2, 3], [3, 4]]
[[1, 2], [2, 3]]
[3, 15, 35]
