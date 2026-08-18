# GAP -- imported from the spinel corpus at c55d9bdb.
# A String transform drops replacement characters ruby emits.
#
# Three more String surfaces from the conformance wave.

# A Hash replacement in sub/gsub looks the match up with #[], so a miss takes
# the hash's DEFAULT. The miss was hard-coded to the empty string.
h = Hash.new("?")
h["e"] = "3"
p "hello".gsub(/[el]/, h)
p "hello".sub(/[el]/, h)

plain = { "e" => "3" }
p "hello".gsub(/[el]/, plain)

# `s[range] = v` with a beginless or endless bound: the sentinel was folded
# through the negative-index fixup into a wild offset (RangeError)
z = +"hello"
z[..1] = "HE"
p z

y = +"hello"
y[2..] = "LLO!"
p y

w = +"hello"
w[1..2] = "X"
p w

v = +"hello"
v[..-4] = "AB"
p v

u = +"hello"
u[...2] = "Q"
p u

# split with a block iterates and answers the receiver, not the split array
p("hello".split(",") { |piece| piece })
out = []
r = "a-b-c".split("-") { |piece| out << piece }
p out
p r
