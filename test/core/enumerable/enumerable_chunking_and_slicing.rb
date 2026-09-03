a = [1, 2, 4, 9, 10, 11, 12, 15]
p a.slice_when { |i, j| i + 1 != j }.to_a
p a.chunk_while { |i, j| i + 1 == j }.to_a
p [1, 1, 2, 3, 3].chunk_while { |i, j| i == j }.to_a
p [1, 2, 3, 4, 5].slice_before { |x| x.even? }.to_a
p [1, 2, 3, 4, 5].slice_after { |x| x.even? }.to_a
p [1, 2, 3, 4, 5].slice_before(3).to_a
p [1, 2, 3, 4, 5].slice_after(3).to_a
p ["a", "b1", "c"].slice_before(/\d/).to_a
__END__
[[1, 2], [4], [9, 10, 11, 12], [15]]
[[1, 2], [4], [9, 10, 11, 12], [15]]
[[1, 1], [2], [3, 3]]
[[1], [2, 3], [4, 5]]
[[1, 2], [3, 4], [5]]
[[1, 2], [3, 4, 5]]
[[1, 2, 3], [4, 5]]
[["a"], ["b1", "c"]]
