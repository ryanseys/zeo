# `map(&:sort)` over rows, and sorting each value of a hash through map.
p [[3, 1, 2], [6, 4, 5]].map(&:sort)
p({ a: [3, 1], b: [2, 4] }.map { |k, v| v.sort })
p [[9, 7, 8]].map { |row| row.sort }
__END__
[[1, 2, 3], [4, 5, 6]]
[[1, 3], [2, 4]]
[[7, 8, 9]]
