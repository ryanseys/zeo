# Integer#chr: no argument yields a single byte; an Encoding encodes the
# codepoint into that encoding.

# No argument: US-ASCII for 0..127, BINARY for 128..255.
p 65.chr
p 65.chr.encoding
p 0.chr.bytes
p 200.chr.bytes
p 200.chr.encoding

# With an Encoding: UTF-8 encodes multibyte, byte encodings stay one byte.
p 233.chr(Encoding::UTF_8)
p 233.chr(Encoding::UTF_8).encoding
p 0x3042.chr(Encoding::UTF_8)
p 233.chr(Encoding::ISO_8859_1).bytes
p 200.chr(Encoding::ASCII_8BIT).bytes

# Out-of-range codepoints raise RangeError.
begin; 256.chr; rescue RangeError => e; puts e.message; end
begin; (-1).chr; rescue RangeError => e; puts e.message; end
begin; 0x110000.chr(Encoding::UTF_8); rescue RangeError => e; puts e.message; end
begin; 300.chr(Encoding::ISO_8859_1); rescue RangeError => e; puts e.message; end
