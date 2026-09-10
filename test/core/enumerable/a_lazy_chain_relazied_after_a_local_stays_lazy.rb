# An infinite prime sieve with an inner scan, re-lazied after the round trip
# through a variable, so lost laziness would be unbounded work.
s = (2..Float::INFINITY).lazy.select { |n| (2...n).none? { |d| n % d == 0 } }
p s.each_cons(2).lazy.first(2)
s2 = (2..30).lazy.select { |n| (2...n).none? { |d| n % d == 0 } }
p s2.each_cons(2).lazy.first(2)
__END__
[[2, 3], [3, 5]]
[[2, 3], [3, 5]]
