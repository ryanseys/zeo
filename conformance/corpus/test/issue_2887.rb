# Iterating a Range twice with a nested-index block (a poly-valued grid read):
# the sum-with-block and map-with-block forms both type correctly (Refs #2913).
grid = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
diag_sum = (0...grid.length).sum { |i| grid[i][i] }
diag = (0...grid.length).map { |i| grid[i][i] }
p diag_sum
p diag
