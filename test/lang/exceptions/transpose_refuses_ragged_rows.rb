# Array#transpose raises IndexError when the rows are not all the same length, and transposes when they are.
r = ([[1, 2], [3]].transpose rescue $!.class); p r
p([[1, 2], [3, 4]].transpose)
p([[1], [2], [3]].transpose)
p([[1, 2, 3], [4, 5, 6]].transpose)
__END__
IndexError
[[1, 3], [2, 4]]
[[1, 2, 3]]
[[1, 4], [2, 5], [3, 6]]
