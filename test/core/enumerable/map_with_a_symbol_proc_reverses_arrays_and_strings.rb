# `map(&:reverse)` over rows, over strings, and over an array holding both.
# (spinel issue #2905)
p [[1, 2], [3, 4]].map(&:reverse)
p ["ab", "cd"].map(&:reverse)
p [[1, 2], "xy"].map(&:reverse)
__END__
[[2, 1], [4, 3]]
["ba", "dc"]
[[2, 1], "yx"]
