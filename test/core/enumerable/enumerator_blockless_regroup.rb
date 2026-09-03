p([1, 2, 3].each.each_slice(2).to_a)
p([1, 2, 3].each.each_cons(2).to_a)
p([1, 2, 3].each.cycle.first(5))
p([1, 2, 3].each.reverse_each.to_a)
p((1..3).each.each_slice(2).to_a)
p([1, 2, 3].each.each_with_index.to_a)
a = [1, "x"]
p(a.cycle.first(5))
__END__
[[1, 2], [3]]
[[1, 2], [2, 3]]
[1, 2, 3, 1, 2]
[3, 2, 1]
[[1, 2], [3]]
[[1, 0], [2, 1], [3, 2]]
[1, "x", 1, "x", 1]
