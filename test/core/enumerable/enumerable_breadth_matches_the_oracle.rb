p [3, 1, 2].sort_by { |x| -x }
p [1, 2, 3, 4].min_by { |x| (x - 3).abs }
p [1, 2, 3, 4].max_by { |x| (x % 3) }
p [3, 1, 2].minmax
p (1..6).group_by { |x| x % 3 }
p [1, 2, 3, 4].partition { |x| x.even? }
p [[1, 2], [3, 4]].flat_map { |a| a }
p [1, 2, 3, 4, 5].filter_map { |x| x * 2 if x.odd? }
acc = []
(1..7).each_slice(3) { |s| acc << s }
p acc
acc2 = []
(1..4).each_cons(2) { |c| acc2 << c }
p acc2
p [1, 2, 3].each_with_object([]) { |x, memo| memo << x * 10 }
p [1, 2, 3, 4].take_while { |x| x < 3 }
p [1, 2, 3, 4].drop_while { |x| x < 3 }
p ["a", "b", "a", "c", "a"].tally
p [1, 2, 2, 3].uniq
p({ a: 1, b: 2 }.sort_by { |k, v| -v })
acc3 = []
[1, 2, 3].reverse_each { |x| acc3 << x }
p acc3
p [[:a, 1], [:b, 2]].to_h
p({ a: 1 }.flat_map { |k, v| [k, v] })
p (1..4).find_index { |x| x > 2 }
__END__
[3, 2, 1]
3
2
[1, 3]
{1 => [1, 4], 2 => [2, 5], 0 => [3, 6]}
[[2, 4], [1, 3]]
[1, 2, 3, 4]
[2, 6, 10]
[[1, 2, 3], [4, 5, 6], [7]]
[[1, 2], [2, 3], [3, 4]]
[10, 20, 30]
[1, 2]
[3, 4]
{"a" => 3, "b" => 1, "c" => 1}
[1, 2, 3]
[[:b, 2], [:a, 1]]
[3, 2, 1]
{a: 1, b: 2}
[:a, 1]
2
