# sum-with-block and map-with-block over a Range, each reading grid[i][i].
# (spinel issue #2887)
grid = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
diag_sum = (0...grid.length).sum { |i| grid[i][i] }
diag = (0...grid.length).map { |i| grid[i][i] }
p diag_sum
p diag
__END__
15
[1, 5, 9]
