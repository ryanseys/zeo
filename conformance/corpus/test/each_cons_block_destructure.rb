# Enumerable#each_cons block destructure on typed arrays:
# `|a, b|` against `each_cons(2)` binds each pair element to
# its own scalar param rather than to a sub-array.

["a", "b", "c", "d"].each_cons(2) { |x, y| puts x + "," + y }
puts "---"
[1, 2, 3, 4, 5].each_cons(2) { |a, b| puts (a + b).to_s }
puts "---"
# `|pair|` (single block param) still gets the sub-array.
[1, 2, 3, 4].each_cons(2) { |pair| puts pair.length }
