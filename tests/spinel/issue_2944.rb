# A nested write g[r][c] = v where r/c are poly (from destructuring) must land
# on the inner array, not be dropped -- sp_poly_set_poly lacked Str/Float array
# cases.
grid = [["a", "b"], ["c", "d"]]
cells = [[0, 1]]
r, c = cells.first
grid[r][c] = "X"
p grid
fg = [[1.0, 2.0]]
fr, fc = [[0, 1]].first
fg[fr][fc] = 9.0
p fg
