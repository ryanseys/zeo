# A lazy chain's uniq, chunk_while and with_index, beside the non-lazy uniq and a lazy map.
p([1, 1, 2, 3, 3].lazy.uniq.to_a)
p([1, 2, 4, 5, 7].lazy.chunk_while { |a, b| b - a == 1 }.to_a)
p([10, 20, 30].lazy.with_index.to_a)
p([1, 1, 2].each.uniq)
p([1, 2, 3].lazy.map { |x| x * 2 }.to_a)
p((1..Float::INFINITY).lazy.select { |x| x % 5 == 0 }.first(2))
__END__
[1, 2, 3]
[[1, 2], [4, 5], [7]]
[[10, 0], [20, 1], [30, 2]]
[1, 2]
[2, 4, 6]
[5, 10]
