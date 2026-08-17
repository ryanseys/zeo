# GAP -- imported from the spinel corpus at c55d9bdb.
# Regexp.last_match with a String name raises "no implicit conversion";
# ruby accepts a name.
#
"2024-01" =~ /(?<y>\d+)-(?<mo>\d+)/
p Regexp.last_match("mo")
p Regexp.last_match(:mo)
p Regexp.last_match("y")
p Regexp.last_match(1)
p Regexp.last_match(2)
p Regexp.last_match(0)
p Regexp.last_match[0]
n = "mo"
p Regexp.last_match(n)
