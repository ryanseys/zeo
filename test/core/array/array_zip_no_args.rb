# Array#zip with nothing to zip against: each element alone in a one-element
# array. Every zip arm was written for at least one argument, so this was
# refused as an unsupported call.
p([1, 2].zip)
p([].zip)
p(["a", "b"].zip)
p([[1], [2]].zip)
p([1.5].zip)

# with arguments, unchanged
p([1, 2].zip([3, 4]))
p([1, 2].zip([3, 4], [5, 6]))
__END__
[[1], [2]]
[]
[["a"], ["b"]]
[[[1]], [[2]]]
[[1.5]]
[[1, 3], [2, 4]]
[[1, 3, 5], [2, 4, 6]]
