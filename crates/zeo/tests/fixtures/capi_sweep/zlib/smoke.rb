require "zlib"
require "stringio"

data = "hello zeo " * 100
d = Zlib::Deflate.deflate(data)
puts d.bytesize < 200, Zlib::Inflate.inflate(d) == data, Zlib.crc32(data), Zlib.adler32(data)
puts Zlib.gunzip(Zlib.gzip("x" * 10))
sio = StringIO.new("".b)
Zlib::GzipWriter.wrap(sio) { |g| g.write("hi there") }
puts Zlib::GzipReader.new(StringIO.new(sio.string)).read
z = Zlib::Deflate.new(Zlib::BEST_COMPRESSION)
out = z.deflate("abc" * 50, Zlib::FINISH)
puts Zlib::Inflate.new.inflate(out), Zlib::ZLIB_VERSION.class
begin
  Zlib::Inflate.inflate("garbage")
rescue Zlib::DataError => e
  puts e.class
end
