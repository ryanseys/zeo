# From the spinel corpus (c55d9bdb).
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
__END__
"01"
"01"
"2024"
"2024"
"01"
"2024-01"
"2024-01"
"01"
