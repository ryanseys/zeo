# MatchData#hash/#eql? are value-based (subject + pattern + regions), so
# two matches produced by separate calls hash equal and work as the same
# Hash key -- while a different subject keys apart.

m1 = "abc".match(/b/)
m2 = "xbz".match(/b/)
m3 = "abc".match(/b/)
p(m1.hash == m2.hash)
p(m1.hash == m3.hash)
p(m1.hash.is_a?(Integer))
h = {}
h[m1] = 99
p h[m3]
p h[m2]
p m1.eql?(m3)
__END__
false
true
true
99
nil
true
