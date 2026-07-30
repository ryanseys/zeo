# `StringIO`'s BYTE-wise reads. Distinct from `getc`'s character-wise ones: a
# multi-byte encoding must not make a byte reader skip. prism's deserializer
# walks its buffer entirely through `getbyte`.
require "stringio"

s = StringIO.new("ab")
p s.getbyte
p s.getbyte
p s.getbyte
p s.eof?

s.rewind
p s.readbyte
p s.pos

begin
  StringIO.new("").readbyte
rescue EOFError => e
  p [e.class, e.message]
end

# A multi-byte character reads one byte at a time here, where `getc` answers
# the whole character.
utf8 = StringIO.new("é")
p utf8.getbyte
p utf8.getbyte
p utf8.getbyte
p StringIO.new("é").getc
