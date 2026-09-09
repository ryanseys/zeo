# Index assignment and `+=` through `m[-1]` and `m[-2]` reach the inner rows,
# for Integer and String elements.
# (spinel issue #3168)
m = [[1, 4], [2, 5]]
m[-1][1] = 99
m[0][0] = 7
m[-2][1] += 100
p m
g = [["a","b"],["c","d"]]
g[-1][0] = "Z"
p g
__END__
[[7, 104], [2, 99]]
[["a", "b"], ["Z", "d"]]
