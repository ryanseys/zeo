# Array#transpose on a nested array swaps rows and columns. Previously
# only int elements were specialized; str and float now mirror it
# (pr_geohash's GeoHash.decode ends with latlng.transpose over floats).
ints = [[1, 2, 3], [4, 5, 6]]
p ints.transpose

floats = [[1.0, 2.0], [3.0, 4.0]]
p floats.transpose
puts floats.transpose[0][0]
puts floats.transpose[1][1]

strs = [["a", "b"], ["c", "d"]]
p strs.transpose

# Square and single-row matrices.
sq = [[1, 2], [3, 4]]
p sq.transpose
one = [["x", "y", "z"]]
p one.transpose
__END__
[[1, 4], [2, 5], [3, 6]]
[[1.0, 3.0], [2.0, 4.0]]
1.0
4.0
[["a", "c"], ["b", "d"]]
[[1, 3], [2, 4]]
[["x"], ["y"], ["z"]]
