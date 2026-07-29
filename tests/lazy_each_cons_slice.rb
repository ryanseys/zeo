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
