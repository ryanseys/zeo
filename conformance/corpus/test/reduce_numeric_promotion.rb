# A fold accumulator seeded with an int but reassigned to a float each
# iteration must accumulate float, not truncate. The seed type alone used to
# fix both the reduce return type and the C accumulator type, so an int seed
# folded over floats silently truncated (`[1.5,2.5].reduce(0)` -> 3, not 4.0).

# int seed, float elements -> float fold
p [1.5, 2.5].reduce(0) { |a, x| a + x }
# 4.0

p [1.5, 2.5, 3.0].inject(0) { |a, x| a + x }
# 7.0

# float seed, int elements -> float fold
p [1, 2, 3].reduce(0.0) { |a, x| a + x }
# 6.0

# int seed, int elements -> stays int (no spurious promotion)
p [1, 2, 3].reduce(0) { |a, x| a + x }
# 6

p [1, 2, 3].reduce(10) { |a, x| a * x }
# 60

# int seed, string elements, int body (x.length) -> stays int
p ["a", "bb", "ccc"].reduce(0) { |a, x| a + x.length }
# 6

# int seed promoted to float via an explicit conversion in the body
p [1, 2, 3].reduce(0) { |a, x| a + x.to_f }
# 6.0

# the chained-map fold from the sibling-collision report
puts([1, 2, 3].map { |x| x + 0.5 }.map { |x| x.floor }.reduce(0) { |a, x| a + x })
# 6
