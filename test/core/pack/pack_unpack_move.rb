# GAP -- imported from the spinel corpus at c55d9bdb.
# String#unpack does not support the X (back up) directive.
#
p "abcdef".unpack("a2X2a4")
p "hello".unpack("@2a*")
r1 = ("abc".unpack("%C3") rescue $!.class)
p r1
p "abcdef".unpack("a2@0a3")
p "abcd".unpack("C2X1C1")
p ["ab"].pack("a2X1")
p [65, 66].pack("C@3C")
p [1, 2, 3].pack("C3")
p [1, 2, 3].pack("C3").bytes
r2 = ("abc".unpack("X5C") rescue $!.class)
p r2
__END__
["ab", "abcd"]
["llo"]
ArgumentError
["ab", "abc"]
[97, 98, 98]
"a"
"A\x00\x00B"
"\x01\x02\x03"
[1, 2, 3]
ArgumentError
