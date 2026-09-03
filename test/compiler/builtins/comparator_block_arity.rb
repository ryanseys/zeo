# A comparator block that takes ONE parameter still gets both elements, so it
# compares the first against nothing and answers a value unrelated to the pair.
# Every pair then ties or contradicts, and the order that comes out is the sort
# algorithm's rather than the program's -- which makes line 7 a pin on the
# algorithm itself. It was a divergence until zeo sorted through the same
# `qsort_r` CRuby's `ruby_qsort` calls.

p [1, 5, 3].max { |x| -x }
p [3, 1, 2].sort { |x| 0 }
p [3, 1, 2].sort { 0 }
p [3, 1, 2].min { |x| 1 }
p [3, 1, 2].sort { |a, b| b <=> a }
p [3, 1, 2].max { |a, b| a <=> b }
p %w[bb a ccc].sort { |x| x.length }
p [2.5, 1.5].min { |x| -1 }
__END__
1
[3, 1, 2]
[3, 1, 2]
3
[3, 2, 1]
3
["ccc", "a", "bb"]
1.5
