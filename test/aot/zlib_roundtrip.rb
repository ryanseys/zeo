# zlib links: deflate and inflate both run in the binary.
require "zlib"
text = "compress me " * 40
packed = Zlib::Deflate.deflate(text)
puts packed.bytesize < text.bytesize
puts Zlib::Inflate.inflate(packed) == text
puts Zlib.crc32("zeo")
__END__
true
true
1671485375
