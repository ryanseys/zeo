p [1, 2, 4, 9, 10, 11, 12, 0].slice_before { |x| x.even? }.to_a
p [1, 2, 4, 9, 10, 11, 12, 0].slice_after { |x| x.even? }.to_a
p %w[a bb c dd].slice_before { |s| s.length == 1 }.to_a
p [3, 1, 4, 1, 5].slice_after { |x| x == 1 }.to_a
e = [1, 2, 3, 4].slice_before { |x| x > 2 }
p e.to_a
__END__
[[1], [2], [4, 9], [10, 11], [12], [0]]
[[1, 2], [4], [9, 10], [11, 12], [0]]
[["a", "bb"], ["c", "dd"]]
[[3, 1], [4, 1], [5]]
[[1, 2], [3], [4]]
