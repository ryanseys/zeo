p([1, 2].lazy.map { |x| [1, 2].lazy.map { |y| y }.to_a }.to_a)
p((1..3).lazy.map { |x| (1..x).lazy.map { |y| y * 2 }.to_a }.to_a)
p((1..3).lazy.map { |x| (1..x).lazy.to_a }.to_a)
p([1, 2].lazy.map { |x| [3, 4].map { |y| y + x } }.to_a)
p((1..4).lazy.select { |x| x.even? }.first(2))
__END__
[[1, 2], [1, 2]]
[[2], [2, 4], [2, 4, 6]]
[[1], [1, 2], [1, 2, 3]]
[[4, 5], [5, 6]]
[2, 4]
