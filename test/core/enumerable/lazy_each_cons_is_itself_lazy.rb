# Enumerator::Lazy#each_cons yields windows on demand, so an infinite source is
# workable and first(n) stops it.
# (spinel issue #3313)
r = ((1..Float::INFINITY).lazy.select { |n| n > 2 }.each_cons(2).first(3) rescue $!.class)
p r
p((1..10).lazy.each_cons(3).first(2))
p((1..Float::INFINITY).lazy.select { |n| n.even? }.each_cons(2).first(2))
p((1..8).lazy.select { |n| n > 3 }.each_cons(2).to_a)
__END__
[[3, 4], [4, 5], [5, 6]]
[[1, 2, 3], [2, 3, 4]]
[[2, 4], [4, 6]]
[[4, 5], [5, 6], [6, 7], [7, 8]]
