# Hash min/max/minmax_by, flat_map splicing, and each_with_object over
# hashes and ranges (the pair/array redispatch family).
h = { a: 1, b: 3, c: 2 }
p h.min
p h.max
p h.minmax_by { |k, v| v }
p h.min_by { |k, v| v }
data = { a: [1, 2], b: [3, 4] }
p data.flat_map { |k, v| v }
p (1..5).each_with_object([]) { |i, acc| acc << i * i }
r = { a: 1, b: 2 }.each_with_object([]) { |(k, v), arr| arr << "#{k}=#{v}" }
p r
__END__
[:a, 1]
[:c, 2]
[[:a, 1], [:b, 3]]
[:a, 1]
[1, 2, 3, 4]
[1, 4, 9, 16, 25]
["a=1", "b=2"]
