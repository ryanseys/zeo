p (1..10).lazy.drop_while { |x| x < 3 }.to_a
p (1..10).lazy.each_slice(2).to_a
p (1..10).lazy.each_slice(3).to_a
p (1..10).lazy.map { |x| x * 2 }.drop_while { |x| x < 9 }.to_a
p (1..10).lazy.select { |x| x.even? }.each_slice(2).to_a
p (1..10).lazy.each_slice(4).first(2)
__END__
[3, 4, 5, 6, 7, 8, 9, 10]
[[1, 2], [3, 4], [5, 6], [7, 8], [9, 10]]
[[1, 2, 3], [4, 5, 6], [7, 8, 9], [10]]
[10, 12, 14, 16, 18, 20]
[[2, 4], [6, 8], [10]]
[[1, 2, 3, 4], [5, 6, 7, 8]]
