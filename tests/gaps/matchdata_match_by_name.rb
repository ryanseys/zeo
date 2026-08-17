# GAP -- imported from the spinel corpus at c55d9bdb.
# Regexp.last_match with a Symbol name raises "no implicit conversion";
# ruby accepts a name.
#
"2024-01-15" =~ /(?<y>\d+)-(?<mo>\d+)-(?<d>\d+)/
p Regexp.last_match(:y)
p Regexp.last_match(:mo)
v001 = Regexp.last_match(:d); p v001
m001 = "2024-01".match(/(?<y>\d+)-(?<mo>\d+)/)
p m001.match(:y)
p m001.match(:mo)
p m001.match_length(:y)
p m001.match_length(:mo)
p m001[:y]
p m001[:mo]
