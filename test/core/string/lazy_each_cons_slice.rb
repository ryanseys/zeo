# `Enumerator::Lazy#each_cons`/`#each_slice` are lazy: an endless source is
# workable, and the chain only advances as far as the terminal asks.
p (1..Float::INFINITY).lazy.each_cons(3).first(2)
p (1..Float::INFINITY).lazy.each_slice(3).first(2)
p (1..10).lazy.each_slice(3).to_a
p (1..7).lazy.each_cons(3).to_a
p (1..2).lazy.each_cons(3).to_a
p (1..10).lazy.each_cons(2).class
p (1..10).lazy.each_cons(2).size
p (1..Float::INFINITY).lazy.each_cons(2).size
p (1..10).lazy.each_slice(3).size
p (1..10).lazy.each_slice(4).size
p (1..3).lazy.each_cons(5).size
p (1..10).lazy.select { |x| x > 1 }.each_cons(2).size
p [1, 2, 3].lazy.each_cons(2).each_cons(2).to_a
p({ a: 1, b: 2, c: 3 }.lazy.each_cons(2).to_a)
p (1..6).lazy.each_cons(2).with_index.first(2)
p (1..6).lazy.each_cons(2).select { |a, b| a.odd? }.to_a
p (1..8).lazy.each_slice(3).map { |g| g.sum }.to_a
p (1..6).lazy.each_cons(0).to_a rescue p $!.message
p (1..6).lazy.each_slice(0).to_a rescue p $!.message

# a block runs the chain at once for its side effects and answers the receiver
seen = []
r = (1..6).lazy.each_cons(2) { |w| seen << w }
p seen
p r.first(3)
__END__
[[1, 2, 3], [2, 3, 4]]
[[1, 2, 3], [4, 5, 6]]
[[1, 2, 3], [4, 5, 6], [7, 8, 9], [10]]
[[1, 2, 3], [2, 3, 4], [3, 4, 5], [4, 5, 6], [5, 6, 7]]
[]
Enumerator::Lazy
9
Infinity
4
3
0
nil
[[[1, 2], [2, 3]]]
[[[:a, 1], [:b, 2]], [[:b, 2], [:c, 3]]]
[[[1, 2], 0], [[2, 3], 1]]
[[1, 2], [3, 4], [5, 6]]
[6, 15, 15]
"invalid size"
"invalid slice size"
[[1, 2], [2, 3], [3, 4], [4, 5], [5, 6]]
[1, 2, 3]
