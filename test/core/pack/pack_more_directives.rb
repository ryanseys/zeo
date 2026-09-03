# Array#pack / String#unpack: float, native-size, quoted-printable, uuencode.

# IEEE-754 floats: D/d/E doubles, G big-endian double, F/f/e single, g BE single.
p [1.5].pack("D").bytes
p [1.5].pack("G").bytes
p [1.5].pack("F").bytes
p [3.14].pack("d").unpack("d")
p [1.0, 2.0, 3.0].pack("E*").unpack("E*")

# Native-size modifiers: l!/L! widen to a native long; i/I and j/J are new.
p [1].pack("l!").bytesize
p [1].pack("i").bytesize
p [1].pack("j").bytesize
p [1].pack("l_").bytesize

# Quoted-printable (M) and uuencode (u), plus their round-trips.
p ["hello world"].pack("M")
p ["a=b c"].pack("M")
p "caf=C3=A9=\n".unpack("M")
p ["hello"].pack("u")
p ["The quick brown fox"].pack("u")
p ["hello world foo"].pack("u").unpack("u")
__END__
[0, 0, 0, 0, 0, 0, 248, 63]
[63, 248, 0, 0, 0, 0, 0, 0]
[0, 0, 192, 63]
[3.14]
[1.0, 2.0, 3.0]
8
4
8
8
"hello world=\n"
"a=3Db c=\n"
["caf\xC3\xA9"]
"%:&5L;&\\`\n"
"35&AE('%U:6-K(&)R;W=N(&9O>```\n"
["hello world foo"]
