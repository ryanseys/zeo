p (1..10).grep(3..5)
p [1, "a", 2, "b"].grep(Integer)
p [1, "a", 2, "b"].grep(Integer) { |x| x * 10 }
p [1, "a", 2, "b"].grep_v(Integer)
p [1, "a", 2, "b"].grep_v(Integer) { |x| x + "!" }
p [1, 2, 3].zip([4, 5, 6])
p [1, 2, 3].zip([4, 5], [6])
p([1, 2, 3].zip([4, 5, 6]) { |x| })
p [1, 2, 3, 4].minmax_by { |x| -x }
p [].minmax_by { |x| x }
__END__
[3, 4, 5]
[1, 2]
[10, 20]
["a", "b"]
["a!", "b!"]
[[1, 4], [2, 5], [3, 6]]
[[1, 4, 6], [2, 5, nil], [3, nil, nil]]
nil
[4, 1]
[nil, nil]
