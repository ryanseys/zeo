# Range#each with no block answers an Enumerator, so `.to_a` and the other
# chained calls work on it.
p((1..3).each.to_a)
p((1...4).each.to_a)
p((5..8).each.to_a.length)
__END__
[1, 2, 3]
[1, 2, 3]
4
