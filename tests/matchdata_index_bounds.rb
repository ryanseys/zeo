# GAP -- imported from the spinel corpus at c55d9bdb.
# MatchData#[] past the last group answers the group text instead of nil.
#
# A negative MatchData index reaches the capture groups only, so the one that
# would land on the whole match is nil. A beginless or endless bound carries
# the range sentinel, which the negative-index fixup turned into a wild offset.
m = "hello".match(/(l)(o)/)
p m[-1]
p m[-2]
p m[-3]
p m[0]
p m[1]
p m[..1]
p m[1..]
p m[0,2]
p m[0..]
n = "abc".match(/(a)(b)(c)/)
p n[-3]
p n[-4]
