# MatchData#values_at with a Range argument wraps the result in an extra
# array.
#
# MatchData#values_at takes a Range, selecting a run of groups the way
# Array#values_at does. The Range went into the group-index slot as a struct,
# so the C did not compile.
m = "hello".match(/(l)(l)(o)/)
p m.values_at(1..2)
p m.values_at(0)
p m.values_at(1,3)
p m.values_at(1...3)
p m.values_at(1..9)
__END__
["l", "l"]
["llo"]
["l", "o"]
["l", "l"]
["l", "l", "o", nil, nil, nil, nil, nil, nil]
