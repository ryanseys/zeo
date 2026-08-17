# GAP -- imported from the spinel corpus at c55d9bdb.
# String#upto across a digit-width boundary answers [] where ruby walks
# "9", "10", "11".
#
# String#upto had no arm at all: it is the String range's own succ-based walk,
# and it answers the receiver. Two all-digit endpoints walk numerically, so
# ("9".."11") holds three elements where a byte compare stopped at once.
z = []
r = "a".upto("c") { |s| z << s }
p z
p r
p "9".upto("11").to_a
p "a".upto("e").to_a
p ("9".."11").to_a
p ("a".."e").to_a
p ("y".."ab").to_a
p ("a"..."d").to_a
w = []
"aa".upto("ac") { |s| w << s }
p w
p 1.upto(3).to_a
q = []
1.upto(3) { |i| q << i }
p q
p "a".upto("e", true).to_a
p "a".upto("e", false).to_a
x = []
"a".upto("d", true) { |s| x << s }
p x
