# The block form yields each match (a String, or an Array of groups) and
# returns the RECEIVER, not the array of matches.

out = []
r = "hello world".scan(/\w+/) { |w| out << w.upcase }
p out
p r
pairs = []
"a1b2".scan(/([a-z])(\d)/) { |l, d| pairs << [l, d] }
p pairs
__END__
["HELLO", "WORLD"]
"hello world"
[["a", "1"], ["b", "2"]]
