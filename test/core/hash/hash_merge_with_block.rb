# Hash#merge with a conflict-resolution block. For a colliding key the block
# is called with (key, this-value, other-value) and its answer is the value
# that lands in the result.

h1 = {a: 1, b: 2}
h2 = {b: 99, c: 3}
result = h1.merge(h2) { |k, v1, v2| v1 + v2 }
puts result[:a]
puts result[:b]   # 2 + 99 = 101
puts result[:c]
__END__
1
101
3
