p([1, nil, 2].lazy.compact.to_a)
p([1, 2, 3].lazy.zip([4, 5, 6]).to_a)
p([1, 2, 3].lazy.map { |x| x * 2 }.to_a)
__END__
[1, 2]
[[1, 4], [2, 5], [3, 6]]
[2, 4, 6]
