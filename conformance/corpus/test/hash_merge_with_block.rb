# Hash#merge with a conflict-resolution block. For colliding keys
# the block sees (key, this-value, other-value); spinel's int
# variant supports it.

h1 = {a: 1, b: 2}
h2 = {b: 99, c: 3}
result = h1.merge(h2) { |k, v1, v2| v1 + v2 }
puts result[:a]
puts result[:b]   # 2 + 99 = 101
puts result[:c]
