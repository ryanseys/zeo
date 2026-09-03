# Zlib rows beyond the (matching) core: an invalid compression level
# raises Zlib::StreamError (zeo compresses anyway); `Zlib.crc32_combine`
# and `Zlib.zlib_version`/`Zlib::VERSION` are absent; and a gzip stream
# with a corrupted ISIZE trailer still reads in ruby where zeo refuses
# with "not in gzip format". (Found by the 2026-08-24 probe sweep.)
require "zlib"
require "stringio"
def show
  p yield
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
show { Zlib::Deflate.deflate("x", 99) }
show { Zlib.crc32_combine(Zlib.crc32("he"), Zlib.crc32("llo"), 3) == Zlib.crc32("hello") }
show { [Zlib.zlib_version.class, Zlib::VERSION.class] }
show do
  io = StringIO.new(+"".b)
  Zlib::GzipWriter.wrap(io) { |g| g.write("x") }
  bad = io.string.dup
  bad[-5] = (bad[-5].ord ^ 0xFF).chr
  Zlib::GzipReader.new(StringIO.new(bad)).read
end
__END__
Zlib::StreamError: stream error
true
[String, String]
"x"
