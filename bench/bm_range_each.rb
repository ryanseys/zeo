# bm_range_each - Range#each over a literal numeric range
#
# Hot loop iterates `(1..500_000_000).each` accumulating
# `sum + (i % 7)` per iteration. The `% 7` introduces a
# sequential data dependency on the loop body so the C
# compiler cannot collapse the loop to a closed form.
#
# Tests Spinel's Range#each lowering: a literal numeric range
# should fuse to a tight C `for` loop with bounds inlined,
# avoiding the `sp_Range` struct allocation+access of the
# generic `each` path.

sum = 0
(1..500_000_000).each { |i| sum = sum + (i % 7) }
puts sum
