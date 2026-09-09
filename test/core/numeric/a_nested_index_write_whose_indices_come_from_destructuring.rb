# `g[r][c] = v` where r and c were bound by a multiple assignment lands on the
# inner array.
# (spinel issue #2944)
grid = [["a", "b"], ["c", "d"]]
cells = [[0, 1]]
r, c = cells.first
grid[r][c] = "X"
p grid
fg = [[1.0, 2.0]]
fr, fc = [[0, 1]].first
fg[fr][fc] = 9.0
p fg
__END__
[["a", "X"], ["c", "d"]]
[[1.0, 9.0]]
