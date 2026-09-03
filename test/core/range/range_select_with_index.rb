# Enumerator chains over an Integer Range source: the with_index family
# materializes the range to an int array and runs the array machinery (#3228).
p((1..10).slice_when { |i, j| j.even? }.to_a)
p((1..8).chunk_while { |i, j| j == i + 1 }.map { |r| r.sum })
p((1..10).select.with_index { |v, i| i.even? })
p((1..5).map.with_index { |v, i| v * i })
p((1..5).select.with_index(1) { |v, i| i.odd? })
p((0..4).reject.with_index { |v, i| i < 2 })
__END__
[[1], [2, 3], [4, 5], [6, 7], [8, 9], [10]]
[36]
[1, 3, 5, 7, 9]
[0, 2, 6, 12, 20]
[1, 3, 5]
[2, 3, 4]
