# GAP -- imported from the spinel corpus at c55d9bdb.
# Range#last on an endless range is evaluated eagerly, so the program never
# terminates and the harness kills it at the 60s deadline.
#
r = ((1..).last(2) rescue $!.class)
p r
p((1..).first(2))
p((1..).min)
p((1..5).last(2))
p((1..5).last)
p((1...5).last(2))
