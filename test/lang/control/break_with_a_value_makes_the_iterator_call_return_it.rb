# `break <v>` in a block makes the whole Enumerable call evaluate to <v>
# (CRuby TAG_BREAK), not the partial accumulator -- across value-producing
# (map/select/reject/reduce/count/find) and self-returning
# (each_with_index) iterators. Bare break -> nil.

a = [1, 2, 3]
p a.map { |x| break 99 if x == 2; x * 10 }
p a.select { |x| break :s if x == 2; x.odd? }
p a.reject { |x| break 7 if x == 2; false }
p a.reduce(0) { |s, x| break 100 if x == 2; s + x }
p a.count { |x| break 5 if x == 2; true }
p a.find { |x| break(-1) if x == 2; false }
p a.each_with_index { |x, i| break i if x == 2 }
p a.map { |x| break if x == 2; x }
p a.map { |x| x + 1 }
__END__
99
:s
7
100
5
-1
1
nil
[2, 3, 4]
