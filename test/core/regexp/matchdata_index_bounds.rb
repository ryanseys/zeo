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
__END__
"o"
"l"
nil
"lo"
"l"
["lo", "l"]
["l", "o"]
["lo", "l"]
["lo", "l", "o"]
"a"
nil
