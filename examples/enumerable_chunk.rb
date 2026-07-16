# The chunk family groups CONSECUTIVE elements of an enumerable.

# `chunk` groups runs that share a block-computed key into [key, [run]] pairs.
runs = [1, 1, 2, 3, 3, 3, 1].chunk { |x| x }.map { |key, group| [key, group.size] }
p runs                                    # [[1, 2], [2, 1], [3, 3], [1, 1]]

# Group numbers by even/odd.
parity = [2, 4, 5, 7, 8].chunk(&:even?).to_a
p parity                                  # [[true, [2, 4]], [false, [5, 7]], [true, [8]]]

# `chunk_while` cuts between two adjacent elements when the block is false --
# here, keeping ascending-by-one runs together.
ascending = [1, 2, 4, 5, 6, 9].chunk_while { |i, j| i + 1 == j }.to_a
p ascending                               # [[1, 2], [4, 5, 6], [9]]

# `slice_when` is its negation: it cuts when the block is TRUE.
gaps = [1, 2, 4, 9, 10, 11, 12, 0].slice_when { |i, j| i + 1 != j }.to_a
p gaps                                    # [[1, 2], [4], [9, 10, 11, 12], [0]]
