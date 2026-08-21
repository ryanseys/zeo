# A `Range` whose endpoint is a BIGNUM is not recognised as an integer
# range, so the operations that need one raise `TypeError: can't iterate
# from Integer` -- a message that names `Integer` as the thing it cannot
# iterate from, which is the tell.
#
# `(2**70..2**70+2)` is three elements and ruby walks it happily. zeo's range
# machinery narrows an endpoint to a machine integer and treats a `BigInt`
# as a non-integer, so `each`, `to_a`, `first(n)`, `last(n)`, `bsearch` and
# `size` all refuse.
#
# The neighbours that DO work say where the line is: `min`, `max`, `step`,
# `include?` and `cover?` all agree with ruby on the same ranges, because
# they compare rather than iterate.
#
# `size` fails a second way and its message is worse: `(1..2**70).size`
# raises `TypeError: no implicit conversion of Integer into Integer`. Ruby
# answers in closed form (`end - begin + 1`), which is also why it is
# instant. A bignum-BEGIN range gets the `can't iterate` message instead, so
# the two endpoints take different paths.
#
# NOT IN THE BODY BELOW, because it does not terminate: `(1..2**70).sum` and
# `(1..2**70).count` HANG. Ruby answers both in closed form -- Gauss for
# `sum`, `size` for `count` -- while zeo iterates, so they are unbounded
# loops rather than divergences. Fixing the range's integer recognition is
# what makes the closed forms reachable.
#
# Oracle: every row answers.
B = 2**70
p (1..B).size
p (B..B + 2).size
p (B..B * 2).size
p (1...B).size
p (1..B).min
p (1..B).max
p (B..B + 5).each.first(2)
p (B..B + 2).to_a
p (B..B + 4).step(2).to_a
p (B..B + 2).include?(B + 1)
p (B..B + 2).cover?(B + 1)
p (B..B + 8).bsearch { |x| x >= B + 3 }
p (B..B + 5).first(2)
p (B..B + 5).last(2)
