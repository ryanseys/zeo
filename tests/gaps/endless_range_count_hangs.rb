# `(1..).count` hangs forever. Ruby answers `Infinity`.
#
# `Range#count` routes to `enumerable::count_own`, which walks `each`. An
# endless range's `each` never stops, so the walk never returns. Ruby special-
# cases a range with no end (and no block/argument) and answers
# `Float::INFINITY` without iterating (`range.c`'s `range_count`).
#
# `size` already answers `Infinity` for the same range, so the information the
# count needs is present -- `count` just does not ask for it. A block or an
# argument DOES have to iterate and legitimately never terminates, which is why
# only the no-argument form can take the shortcut.

r = (1..)
p r.size
p r.count
