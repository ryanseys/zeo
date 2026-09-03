# str.match(re) types as MatchData but returns nil on no match, so .class
# must be read at runtime rather than constant-folded to MatchData.

p "hi".match(/h/).class
p "hi".match(/z/).class
m = "hi".match(/z/)
p m.class
__END__
MatchData
NilClass
NilClass
