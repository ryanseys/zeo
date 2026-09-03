p [[1, 2, 3], [4, 5, 6]].map(&:reverse).transpose
p [[1, 2], [3, 4]].transpose
p [["a", "b"], ["c", "d"]].transpose
p [[1, 2, 3], [4, 5, 6], [7, 8, 9]].map(&:reverse).transpose
__END__
[[3, 6], [2, 5], [1, 4]]
[[1, 3], [2, 4]]
[["a", "c"], ["b", "d"]]
[[3, 6, 9], [2, 5, 8], [1, 4, 7]]
