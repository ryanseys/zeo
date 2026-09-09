# ("a".."c") iterates, selects and maps its members.
# (spinel issue #3103)
r = []
("a".."c").each { |x| r << x }
p r
p ("a".."e").select { |s| s > "b" }
p ("a".."c").map { |s| s.upcase }
p ("a".."d").find { |s| s == "c" }
p ("aa".."ac").to_a
# int and float ranges unchanged
sum = 0
(1..5).each { |n| sum += n }
p sum
p (1..3).map { |n| n * 2 }
fr = []
(1.0..3.0).step(1.0) { |x| fr << x }
p fr
__END__
["a", "b", "c"]
["c", "d", "e"]
["A", "B", "C"]
"c"
["aa", "ab", "ac"]
15
[2, 4, 6]
[1.0, 2.0, 3.0]
