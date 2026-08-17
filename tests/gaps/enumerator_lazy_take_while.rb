# GAP -- imported from the spinel corpus at c55d9bdb.
# Enumerator::Lazy#take_while is not lazy over an endless source, so the
# program never terminates and the harness kills it at the 60s deadline.
#
p(Enumerator.produce(1) { |n| n + 1 }.take_while { |n| n < 5 })
p(Enumerator.new { |y| i = 0; loop { y << (i += 1) } }.take_while { |n| n < 4 })
p([1, 2, 3].each.take_while { |n| n < 3 })
p(Enumerator.produce(1) { |n| n + 1 }.find { |n| n > 3 })
p([1, 2, 3].each.take_while { |n| n > 9 })
