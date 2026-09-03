require "zlib"
puts Zlib.crc32("abc")
puts Zlib.adler32("abc")
puts Zlib.crc32("abc", 100)
__END__
891568578
38600999
2063213118
