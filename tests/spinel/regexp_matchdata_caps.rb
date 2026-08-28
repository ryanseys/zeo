# MatchData carries its capture positions in the same block as the struct,
# sized to the match. The read of the last group is what a mis-sized block
# would get wrong, so the widest pattern this engine allows is exercised here
# along with the narrow ones.
m = "hello world".match(/(\w+) (\w+)/)
p [m.size, m[0], m[1], m[2], m[3]]
p m.pre_match
p m.post_match
p m.begin(0)
p m.begin(2)
p m.to_a
p m.captures

# no groups at all: the block is just the whole-match pair
m0 = "abc".match(/b/)
p [m0.size, m0[0], m0[1]]

# an unmatched group in the middle keeps its neighbours' positions
m1 = "ac".match(/(a)(b)?(c)/)
p [m1.size, m1[1], m1[2], m1[3]]
p m1.to_a

# the widest pattern the registers hold: every group must read back
src = (1..31).map { "(a)" }.join
w = ("a" * 31).match(Regexp.new(src))
p w.size
p [w[1], w[15], w[30], w[31]]
p w.captures.length
p w.captures.uniq
p w.begin(31)
p w.end(31)

# named groups reach the same positions
n = "2026-08-25".match(/(?<y>\d+)-(?<mo>\d+)-(?<d>\d+)/)
p [n[:y], n[:mo], n[:d]]
p n.names
p n.named_captures

# $~ goes through the same block
"xyz" =~ /(y)(z)/
p [$~.size, $~[1], $~[2], $1, $2]
