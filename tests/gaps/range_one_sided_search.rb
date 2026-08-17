# GAP -- imported from the spinel corpus at c55d9bdb.
# A search over a one-sided Range is evaluated eagerly, so the program
# never terminates and the harness kills it at the 60s deadline.
#
p((1..).find { |x001| x001 * x001 > 30 })
p((1..).detect { |x002| x002 > 3 })
p((1..).take_while { |x004| x004 < 4 })
p((1..5).all?(Integer))
p((1..5).any?(2..4))
p((1..5).none?(String))
p((1..5).find { |x| x > 3 })
p((1..5).take_while { |x| x < 3 })
r = (1..)
p r.find { |x| x > 7 }
