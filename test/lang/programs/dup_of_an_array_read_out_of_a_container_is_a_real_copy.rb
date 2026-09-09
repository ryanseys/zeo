# Writing to the copy leaves the original row alone.
# (spinel issue #2946)
board = [["a", "b"], ["c", "d"]]
copy = board[0].dup
copy[1] = "X"
p board[0]
p copy
ints = [[1, 2], [3, 4]]
c = ints[0].dup
c[1] = 99
p ints[0]
p c
__END__
["a", "b"]
["a", "X"]
[1, 2]
[1, 99]
