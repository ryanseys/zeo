# The encoding tables are linked: transcoding, byte inspection and the
# encoding registry all answer in the binary.
s = "héllo wörld"
puts s.encoding
latin = s.encode("ISO-8859-1")
puts latin.bytesize
puts latin.encode("UTF-8")
p "\xC3\xA9".dup.force_encoding("UTF-8").valid_encoding?
p "\xFF".dup.force_encoding("UTF-8").valid_encoding?
puts Encoding::UTF_8.name
__END__
UTF-8
11
héllo wörld
true
false
UTF-8
