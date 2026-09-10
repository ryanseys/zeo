# A block answering a scalar, an array, either by condition, and the
# collect_concat alias.
p (1..3).flat_map { |x| x * 2 }
p (1..3).flat_map { |x| [x, x] }
p (1..4).flat_map { |x| x.even? ? [x, x] : x }
p ("a".."c").flat_map { |s| s.upcase }
p (1..3).collect_concat { |x| x + 1 }
p [10, 20].flat_map { |x| x / 10 }
__END__
[2, 4, 6]
[1, 1, 2, 2, 3, 3]
[1, 2, 2, 3, 4, 4]
["A", "B", "C"]
[2, 3, 4]
[1, 2]
