# Array#permutation on integer arrays, mirroring combination: blockless returns
# an array of the k-element permutations; the block form yields each one.
p [1, 2, 3].permutation(2).to_a
p [1, 2, 3].permutation.to_a          # argless = full-length permutations
p [1, 2, 3].permutation(0).to_a       # one empty permutation
p [1, 2].permutation(5).to_a          # k > length -> none
p [1, 2, 3].permutation(2).map { |pr| pr.sum }
acc = []
[1, 2, 3].permutation(2) { |pr| acc << pr.sum }
p acc
__END__
[[1, 2], [1, 3], [2, 1], [2, 3], [3, 1], [3, 2]]
[[1, 2, 3], [1, 3, 2], [2, 1, 3], [2, 3, 1], [3, 1, 2], [3, 2, 1]]
[[]]
[]
[3, 4, 3, 5, 4, 5]
[3, 4, 3, 5, 4, 5]
