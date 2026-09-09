# Each materializes the pairs and auto-splats them into the block.
# (spinel issue #2943)
a = [3, 1, 2]
p a.each_with_index.max_by { |v, _i| v }
p a.each_with_index.min_by { |v, _i| v }
p a.each_with_index.sort_by { |v, _i| v }
p a.each_with_index.sum { |v, _i| v }
__END__
[3, 0]
[1, 1]
[[1, 1], [2, 2], [3, 0]]
6
