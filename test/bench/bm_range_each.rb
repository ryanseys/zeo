# bm_range_each - Range#each over a literal numeric range
#
# Hot loop iterates `(1..500_000_000).each` accumulating
# `sum + (i % 7)` per iteration. The `% 7` introduces a
# sequential data dependency on the loop body so the C
# compiler cannot collapse the loop to a closed form.
#
# What this times is Range#each over a literal numeric range, which should
# come out as a tight counted loop with its bounds inlined, rather than the
# object allocation and field reads of the
# generic `each` path.

sum = 0
(1..500_000_000).each { |i| sum = sum + (i % 7) }
puts sum
__END__
1499999997
