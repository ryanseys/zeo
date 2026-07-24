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
