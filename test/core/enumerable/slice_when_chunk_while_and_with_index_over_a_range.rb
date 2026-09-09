# slice_when and chunk_while grouping consecutive members, and with_index with
# and without an offset.
# (spinel issue #3228)
p((1..10).slice_when { |i, j| j.even? }.to_a)
p((1..8).chunk_while { |i, j| j == i + 1 }.map { |r| r.sum })
p((1..8).chunk_while { |i, j| j == i + 1 }.to_a)
p((1..4).each.with_index(5).to_a)
p((1..4).each.with_index.to_a)
p((1..4).map.with_index { |v, i| v * i })
__END__
[[1], [2, 3], [4, 5], [6, 7], [8, 9], [10]]
[36]
[[1, 2, 3, 4, 5, 6, 7, 8]]
[[1, 5], [2, 6], [3, 7], [4, 8]]
[[1, 0], [2, 1], [3, 2], [4, 3]]
[0, 2, 6, 12]
