# `"".b << 181` is the single raw byte 0xB5; a UTF-8 receiver encodes
# the codepoint (an emoji is 4 bytes); a binary append composes.

b = "".b
b << 181
p b.bytes
u = +"xy"
u << 0x1F600
p u.bytesize
c = 180.chr
c << 5.chr
p c.bytes
__END__
[181]
6
[180, 5]
