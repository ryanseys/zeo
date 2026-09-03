# The short-circuiting Enumerable methods stop the SOURCE, not just their own
# scan, so they answer over an endless one. Each of these used to collect the
# whole source first and so never returned.
#
# `find_index` is the one with no spinel coverage: it asks the same question as
# `find`, which always stopped early, so the two disagreeing on an endless
# source was a bug rather than a stated limitation.

# take_while over an endless Range, an Enumerator, and a user Enumerable.
p((1..).take_while { |x| x < 4 })
p(Enumerator.produce(1) { |n| n + 1 }.take_while { |n| n < 5 })

class Inf
  include Enumerable
  def each
    i = 1
    loop { yield i; i += 1 }
  end
end
p(Inf.new.take_while { |x| x < 3 })

# find_index, both forms, on an endless source.
p((1..).find_index { |x| x * x > 30 })
p((10..).find_index(13))
p(Inf.new.find_index { |x| x > 2 })

# ... and the finite answers stay the same, including the misses.
p([1, 2, 3].find_index { |x| x > 9 })
p([1, 2, 3].find_index(9))
p([5, 6, 7].find_index(6))

# each_slice / each_cons roll a buffer instead of materializing the source.
p((1..).each_slice(2).first(2))
p((1..).each_cons(3).first(2))
p((1..).each_slice(3).first(1))

# A short final slice still yields; a source shorter than the window yields
# nothing at all.
p([1, 2, 3, 4, 5].each_slice(2).to_a)
p([1, 2].each_cons(5).to_a)
p([1, 2, 3, 4].each_cons(2).to_a)

# drop_while stays eager (its answer is the tail) and keeps its answer.
p([1, 2, 3, 1, 2].drop_while { |x| x < 3 })
p([1, 2, 3].drop_while { |x| x > 9 })

# A `break` in the predicate belongs to the CALLER, and must not be read as the
# internal stop these methods use to halt the source.
p([1, 2, 3].take_while { |x| break :broke_tw })
p([1, 2, 3].find_index { |x| break :broke_fi })
p([1, 2, 3, 4].each_slice(2) { |s| break s })
p([1, 2, 3, 4].each_cons(2) { |c| break c })
p((1..).take_while { |x| break :endless_break })

# The block forms answer the receiver.
p([1, 2, 3, 4].each_slice(2) { |_s| nil })
p([1, 2, 3, 4].each_cons(2) { |_c| nil })
__END__
[1, 2, 3]
[1, 2, 3, 4]
[1, 2]
5
3
2
nil
nil
1
[[1, 2], [3, 4]]
[[1, 2, 3], [2, 3, 4]]
[[1, 2, 3]]
[[1, 2], [3, 4], [5]]
[]
[[1, 2], [2, 3], [3, 4]]
[3, 1, 2]
[1, 2, 3]
:broke_tw
:broke_fi
[1, 2]
[1, 2]
:endless_break
[1, 2, 3, 4]
[1, 2, 3, 4]
