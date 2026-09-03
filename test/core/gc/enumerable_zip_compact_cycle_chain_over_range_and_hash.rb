p (1..3).zip([4, 5, 6], [7, 8, 9])
p (1..5).compact
p (1..3).chain([4, 5]).to_a
seen = []
(1..3).cycle(2) { |x| seen << x }
p seen
p({ a: 1, b: 2 }.zip([10, 20]))
p({ a: 1 }.rehash)
p "hello".tr_s("l", "r")
p "aabbcc".tr_s("a-c", "x")
s = "hello"
p s.tr_s!("l", "r")
p s
p "clean".scrub!
__END__
[[1, 4, 7], [2, 5, 8], [3, 6, 9]]
[1, 2, 3, 4, 5]
[1, 2, 3, 4, 5]
[1, 2, 3, 1, 2, 3]
[[[:a, 1], 10], [[:b, 2], 20]]
{a: 1}
"hero"
"x"
"hero"
"hero"
"clean"
