# A single-space separator selects Ruby's whitespace mode even when the
# separator is held in a variable or an explicit limit is supplied.
sep = " "
p " one two  three ".split(sep)
p " one two  three ".split(sep, 0)
p " one two  three ".split(sep, 1)
p " one two  three ".split(sep, 2)
p " one two  three ".split(sep, -1)
p "   ".split(sep, 0)
p "   ".split(sep, 2)
p "   ".split(sep, -1)
p "trail   ".split(sep, 3)

out = []
"  a   b c ".split(sep, 2) { |piece| out << piece }
p out
