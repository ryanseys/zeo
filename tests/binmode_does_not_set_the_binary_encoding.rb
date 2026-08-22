# `#binmode` returns the stream and changes nothing. In ruby it puts the
# stream in binary mode, which sets the external encoding to `ASCII-8BIT`,
# and everything read from it comes back binary.
#
#   StringIO.new("a").binmode.external_encoding   ruby ASCII-8BIT  zeo UTF-8
#   File#binmode                                  the same
#
# On a platform with no newline translation the OTHER half of binary mode is
# a no-op, which is presumably why this looked harmless: nothing about the
# BYTES changes. The encoding does, and that is observable everywhere the
# bytes are later interpreted -- a string written to a binmode stream comes
# back tagged UTF-8 in zeo, so a caller that binmodes a stream precisely to
# stop it interpreting bytes still gets them interpreted.
#
# `File.binread` already answers ASCII-8BIT, so the encoding-carrying
# machinery is there; `binmode` simply does not reach it.
#
# Oracle: binmode means binary.
require "stringio"
require "tempfile"
p StringIO.new("a").binmode.external_encoding.to_s
s = StringIO.new("".dup)
s.binmode
s.write("é")
p s.string.encoding.to_s
f = Tempfile.new("bm")
f.binmode
p f.external_encoding.to_s
f.close!
