# Enumerator#compact drops the nils and #zip pairs against another array.
p([1, nil, 2, nil].each.compact)
p([1, 2, 3].each.zip([4, 5, 6]))
__END__
[1, 2]
[[1, 4], [2, 5], [3, 6]]
