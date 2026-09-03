# GAP -- imported from the spinel corpus at c55d9bdb.
# MatchData#frozen? answers false; ruby freezes MatchData.
#
m001 = "hello".match(/l+/)
p m001.frozen?
p m001.to_a.frozen?
p m001.string.frozen?
__END__
false
false
true
