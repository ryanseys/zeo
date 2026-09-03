# A String pattern is a literal (no metacharacters), matches
# non-overlapping, and an empty pattern matches at every character
# boundary. The block form yields each match and returns the receiver.

p "MixedCase".scan("e")
p "aaaa".scan("aa")
p "abc".scan("z")
p "café".scan("")
matches = []
returned = "banana".scan("an") { |m| matches << m.upcase }
p matches
p returned == "banana"
__END__
["e", "e"]
["aa", "aa"]
[]
["", "", "", "", ""]
["AN", "AN"]
true
