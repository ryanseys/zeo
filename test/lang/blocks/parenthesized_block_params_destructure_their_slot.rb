# A parenthesized parameter destructures the value bound to its slot --
# `|(a, b)|`, mixed with plain params, and nested arbitrarily deep. Lowered
# as the multi-assignment it is (see `hir::Params::destructures`).

[[1, 2], [3, 4]].each { |(a, b)| p [a, b] }
[[1, [2, 3], 4]].each { |a, (b, c), d| p [a, b, c, d] }
[[[1, 2], 3]].each { |(a, b), c| p [a, b, c] }
[[1, [2, [3, 4]]]].each { |a, (b, (c, d))| p [a, b, c, d] }
__END__
[1, 2]
[3, 4]
[1, 2, 3, 4]
[1, 2, 3]
[1, 2, 3, 4]
