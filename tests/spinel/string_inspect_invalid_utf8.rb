# String#inspect escapes a byte that is not part of a valid UTF-8 sequence as
# \xNN, the way CRuby renders it. Passing the raw byte through made the inspect
# output itself invalid UTF-8, so a terminal drew U+FFFD -- inspect lost the one
# thing it was being asked about. Found while sweeping the nil-sentinel receiver
# surface, where 128.chr.inspect was the odd answer out.

# invalid bytes, alone and embedded
p "a\x80b"
p "a\xffb"
p "\xc3"
p "\xe3\x81"
p "\x80"
p "\xff\xfe"

# valid sequences still pass through unchanged
p "café"
p "é"
p "日本語"
p "\u{1F600}"
p "abc"
p "a\tb\nc"

# a BINARY string escapes every high byte
p [200, 300].pack("C*")
p "abc".b

# the escaped form does not change the string itself
p "a\x80b".bytes
p "a\x80b".bytesize
p "café".bytes
