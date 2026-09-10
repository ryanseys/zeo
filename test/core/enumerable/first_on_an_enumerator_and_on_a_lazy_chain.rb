# Enumerator#first with and without a count, on each, each_with_index, a lazy map, and an empty array.
p([1, 2, 3].each.first)
p([1, 2, 3].each.first(2))
p([1, 2, 3].each_with_index.first)
p([1, 2, 3].lazy.map { |x| x * 2 }.first)
p([].lazy.map { |x| x }.first)
p((1..Float::INFINITY).lazy.select { |x| x % 7 == 0 }.first)
__END__
1
[1, 2]
[1, 0]
2
nil
7
