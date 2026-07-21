# Array#dup of an array read out of a poly container must be a real copy, not
# an alias -- mutating the copy must not corrupt the original.
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
