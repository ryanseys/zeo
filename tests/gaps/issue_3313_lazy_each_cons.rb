# `each_cons` on a lazy enumerator over an infinite range is not itself lazy
# here: it materializes the source instead of yielding windows on demand, so
# the program never terminates and the harness kills it at 60s. ruby's
# `Enumerator::Lazy#each_cons` stays lazy and `first(n)` stops the source.
r = ((1..Float::INFINITY).lazy.select { |n| n > 2 }.each_cons(2).first(3) rescue $!.class)
p r
p((1..10).lazy.each_cons(3).first(2))
p((1..Float::INFINITY).lazy.select { |n| n.even? }.each_cons(2).first(2))
p((1..8).lazy.select { |n| n > 3 }.each_cons(2).to_a)
