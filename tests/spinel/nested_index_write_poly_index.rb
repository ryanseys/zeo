# `rows[r][c] = "*"` where `r` comes from a destructured block parameter. The
# store has two lowerings: one that writes through the outer container's slot,
# and one that fetches the element by value and writes into that copy. The
# addressable test required an INTEGER index, so a boxed one fell to the
# by-value form and the assignment was silently dropped -- no error, and the
# expression still answered its right-hand side (#4078).
rows = ["abc"].map(&:dup)
pairs = [[0, 1]]
pairs.each { |r, c| rows[r][c] = "*" }
p rows

# the shapes that already worked, so they still do
ok1 = ["abc"].map(&:dup)
[[0, 1]].each { |r, c| ok1[r][c] = "*" }
p ok1

ok2 = ["abc"].map(&:dup)
idx = [1]
idx.each { |c| ok2[0][c] = "*" }
p ok2

# more than one pair, and a second row
many = ["abc", "def"].map(&:dup)
[[0, 1], [1, 0]].each { |r, c| many[r][c] = "*" }
p many

# an Integer array inner, which takes the widen-and-store path
grid = [[1, 2], [3, 4]]
gp = [[0, 1]]
gp.each { |r, c| grid[r][c] = 9 }
p grid

# a Hash outer keyed by a String: the slot helpers address by integer, so this
# one must NOT take the addressable path
h = { "k" => [1, 2] }
kk = [["k", 0]]
kk.each { |k, i| h[k][i] = 7 }
p h

# three levels deep
deep = [[["a"]]]
[[0, 0]].each { |x, y| deep[x][y][0] = "b" }
p deep
