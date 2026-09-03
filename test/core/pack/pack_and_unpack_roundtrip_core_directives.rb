# Array#pack / String#unpack across the integer, string, base64, hex,
# BER and UTF-8 directives. Verified against ruby 4.0.6.

p [65, 66, 67].pack("C*")
p [258].pack("v").bytes
p [258].pack("S>").bytes
p [-1].pack("l").bytes
p "\x00\x00\x00\x01".unpack("N")
p "\xff\xff\xff\xff".unpack("l")
p "\xff\xff\xff\xff".unpack("L")
p ["hi"].pack("a5").bytes
p ["hi"].pack("A5").bytes
p "abc\0de".unpack("Z*")
p ["hello world"].pack("m")
p "aGVsbG8=\n".unpack("m")
p ["ff01"].pack("H*").bytes
p [300].pack("w").bytes
p [12354].pack("U")
p "あ".unpack("U*")
p [65].pack("C").encoding
p [12354].pack("U").encoding
__END__
"ABC"
[2, 1]
[1, 2]
[255, 255, 255, 255]
[1]
[-1]
[4294967295]
[104, 105, 0, 0, 0]
[104, 105, 32, 32, 32]
["abc"]
"aGVsbG8gd29ybGQ=\n"
["hello"]
[255, 1]
[130, 44]
"あ"
[12354]
#<Encoding:BINARY (ASCII-8BIT)>
#<Encoding:UTF-8>
