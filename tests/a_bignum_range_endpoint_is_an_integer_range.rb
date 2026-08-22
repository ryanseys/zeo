# A `Range` whose endpoint is a BIGNUM is an integer range like any other.
#
# zeo's range machinery narrowed every endpoint to a machine integer and
# treated a `BigInt` as a non-integer, so `each`, `to_a`, `first(n)`,
# `last(n)`, `bsearch` and `size` all raised `TypeError: can't iterate from
# Integer` -- a message that names `Integer` as the thing it cannot iterate
# from, which was the tell. The neighbours that DID work said where the line
# was: `min`, `max`, `step`, `include?` and `cover?` all agreed already,
# because they compare rather than iterate.
#
# `big_endpoint` is the shared view -- an integer endpoint at full width,
# whichever the value arrived in -- and `big_in_range` its bound test. The
# counter decides the width, not the endpoint, so `(2**70..2**70+2)` narrows
# back down as it goes.
#
# Two rows were WORSE than a divergence: `(1..2**70).sum` and `.count` HUNG,
# because zeo iterated where ruby answers in closed form. Those closed forms
# are why ruby answers at all -- Gauss for `sum`, `size` for `count` -- and
# `size` is the same story (`end - begin + 1`, which is also why it is
# instant).

B = 2**70

p [(1..B).size, (B..B + 2).size, (B..B * 2).size, (1...B).size]
# An empty range at full width is 0, not a negative count.
p [(B...B).size, (B..B - 1).size]
p [(1..B).min, (1..B).max, (B..B + 5).min, (B..B + 5).max]

p (B..B + 5).each.first(2)
p (B..B + 2).to_a
p (B..B + 4).step(2).to_a
p [(B..B + 2).include?(B + 1), (B..B + 2).cover?(B + 1), (B..B + 2).include?(B + 9)]

# Both bsearch modes, and the no-hit answer.
p (B..B + 8).bsearch { |x| x >= B + 3 }
p (B..B + 8).bsearch { |x| B + 3 <=> x }
p (B..B + 8).bsearch { false }

p [(B..B + 5).first(2), (B..B + 5).last(2), (B..B + 5).first, (B..B + 5).last]
p (B..B + 3).map { |x| x - B }
p (B..B + 3).select(&:even?).map { |x| x - B }
p (B..B + 3).each_slice(2).to_a.map { |a| a.map { |x| x - B } }
p ((-B)..(-B + 2)).to_a.map { |x| x + B }
p (B..).first(2)
p (1..B).lazy.map { |x| x * 2 }.first(3)

# The closed forms, which are what makes these terminate at all.
p (1..B).sum
p (1..B).count
p (1...B).sum
p (B..B + 3).sum - 4 * B
p (B..B + 3).count

# ... and the shapes that must still walk.
p [(1..5).sum, (1..5).sum(10), (1..5).sum { |x| x * 2 }, (1..5).sum(0.0)]
p [(5..1).sum, (5..1).count]
p ("a".."c").sum("")
p [(1..3).count { |x| x > 1 }, (1..3).count(2)]
