# zlib's class surface: the incremental `ZStream` pair and the IO-shaped gzip
# pair. Nothing here prints a timestamp or a whole compressed buffer -- gzip
# headers carry the current time, and deflate output is only guaranteed to
# ROUND-TRIP, not to be byte-identical across zlib implementations.
require "zlib"
require "stringio"

def show(label)
  print "#{label}: "
  p(yield)
rescue => e
  puts "#{e.class}: #{e.message}"
end

TEXT = "the quick brown fox jumps over the lazy dog. " * 4

# -- Deflate/Inflate: the incremental surface -------------------------------

# `deflate` hands back what has been FLUSHED so far, which with the default
# NO_FLUSH is usually nothing; `finish` drains the rest.
show("streamed round-trip") do
  z = Zlib::Deflate.new
  parts = [z.deflate("hello "), z.deflate("world"), z.finish]
  Zlib::Inflate.inflate(parts.join)
end
# `<<` queues its output instead of handing it back, so the whole stream
# arrives on the next detaching call.
show("shovel queues") do
  z = Zlib::Deflate.new
  z << "hello " << "world"
  Zlib::Inflate.inflate(z.finish)
end
show("sync flush mid-stream") do
  z = Zlib::Deflate.new
  first = z.deflate("hello", Zlib::SYNC_FLUSH)
  [first.empty?, Zlib::Inflate.inflate(first + z.finish)]
end
show("one-shot class methods") { Zlib::Inflate.inflate(Zlib::Deflate.deflate(TEXT)) == TEXT }
show("compressed encoding") { Zlib::Deflate.deflate("hi").encoding.to_s }
show("inflated encoding") { Zlib::Inflate.inflate(Zlib.deflate("hi")).encoding.to_s }

# Window bits pick the container, and are the reason `Net::HTTP` can decode a
# body before it knows which encoding arrived.
show("raw deflate") do
  raw = Zlib::Deflate.new(9, -Zlib::MAX_WBITS).deflate("hi", Zlib::FINISH)
  [raw[0].unpack1("C"), Zlib::Inflate.new(-Zlib::MAX_WBITS).inflate(raw)]
end
show("gzip via window bits") do
  gz = Zlib::Deflate.new(9, 16 + Zlib::MAX_WBITS).deflate("hi", Zlib::FINISH)
  [gz.bytes.first(2), Zlib.gunzip(gz)]
end
show("auto-detect gzip") { Zlib::Inflate.new(32 + Zlib::MAX_WBITS).inflate(Zlib.gzip("hello")) }
show("auto-detect zlib") { Zlib::Inflate.new(32 + Zlib::MAX_WBITS).inflate(Zlib.deflate("hello")) }
show("split across writes") do
  gz = Zlib.gzip(TEXT)
  z = Zlib::Inflate.new(32 + Zlib::MAX_WBITS)
  out = +""
  gz.each_byte { |b| out << z.inflate(b.chr) }
  out == TEXT
end

# The counters and lifecycle `ZStream` shares.
show("counters") do
  z = Zlib::Deflate.new
  z.deflate("hello world", Zlib::FINISH)
  [z.total_in, z.adler, z.data_type, z.avail_in, z.finished?, z.closed?]
end
show("avail_out is a knob") do
  z = Zlib::Deflate.new
  z.avail_out = 100
  z.avail_out
end
# Only the CONTENT is asserted, not whether the queue had anything in it yet:
# zlib emits the 2-byte stream header as soon as `<<` runs, miniz_oxide holds
# it until there is compressed data to go with it. Both produce the same
# stream (see docs/COMPATIBILITY.md).
show("flush_next_out drains") do
  z = Zlib::Deflate.new
  z << "hello"
  Zlib::Inflate.inflate(z.flush_next_out + z.finish)
end
show("reset starts over") do
  z = Zlib::Deflate.new
  z.deflate("discarded")
  z.reset
  [Zlib::Inflate.inflate(z.deflate("kept", Zlib::FINISH)), z.total_in]
end
show("close then use") do
  z = Zlib::Deflate.new
  z.finish
  z.close
  [z.closed?, (z.deflate("x") rescue "#{$!.class}")]
end
show("deflate after finish") do
  z = Zlib::Deflate.new
  z.finish
  z.deflate("x")
end
show("finish twice") { z = Zlib::Deflate.new; z.finish; z.finish }

# Failure modes, each its own exception class.
show("not compressed at all") { Zlib::Inflate.inflate("not compressed at all") }
show("truncated stream") { Zlib::Inflate.inflate(Zlib.deflate(TEXT)[0..5]) }
show("trailing garbage") do
  z = Zlib::Inflate.new
  [z.inflate(Zlib.deflate("hi") + "EXTRA"), z.finished?]
end
show("sync_point?") { Zlib::Inflate.new.sync_point? }

# -- GzipWriter/GzipReader --------------------------------------------------

GZ = Zlib.gzip("hello\nworld\n")

show("writer round-trip") do
  io = StringIO.new("".b)
  w = Zlib::GzipWriter.new(io)
  w.write("hello ")
  w << "world"
  w.close
  Zlib.gunzip(io.string)
end
show("writer close answers the io") do
  io = StringIO.new("".b)
  Zlib::GzipWriter.new(io).close.equal?(io)
end
show("writer wrap answers the block") do
  io = StringIO.new("".b)
  value = Zlib::GzipWriter.wrap(io) { |w| w.write("wrapped"); 42 }
  [value, Zlib.gunzip(io.string)]
end
show("writer header fields") do
  io = StringIO.new("".b)
  w = Zlib::GzipWriter.new(io)
  w.mtime = 1234567890
  w.orig_name = "f.txt"
  w.comment = "a note"
  w.write("x")
  w.close
  r = Zlib::GzipReader.new(StringIO.new(io.string))
  [r.orig_name, r.comment, r.mtime.to_i, r.os_code, r.read]
end
show("writer header is sealed by the first write") do
  io = StringIO.new("".b)
  w = Zlib::GzipWriter.new(io)
  w.write("x")
  w.orig_name = "too late"
end
show("writer print surface") do
  io = StringIO.new("".b)
  w = Zlib::GzipWriter.new(io)
  w.puts("a")
  w.print("b", "c")
  w.printf("%03d", 7)
  w.putc(?Z)
  w.close
  Zlib.gunzip(io.string)
end
show("writer pos and write") do
  io = StringIO.new("".b)
  w = Zlib::GzipWriter.new(io)
  n = w.write("hello")
  r = [n, w.pos, w.tell, w.crc, w.to_io.equal?(io), w.closed?]
  w.close
  r << w.closed?
end
show("writer level in the header") do
  io = StringIO.new("".b)
  Zlib::GzipWriter.wrap(io, Zlib::BEST_COMPRESSION) { |w| w.write(TEXT) }
  Zlib::GzipReader.new(StringIO.new(io.string)).level
end
show("writer accessors after close") do
  io = StringIO.new("".b)
  w = Zlib::GzipWriter.new(io)
  w.close
  w.crc
end

show("reader read") { Zlib::GzipReader.new(StringIO.new(GZ)).read }
show("reader read encoding") { Zlib::GzipReader.new(StringIO.new(GZ)).read.encoding.to_s }
show("reader read(n) encoding") { Zlib::GzipReader.new(StringIO.new(GZ)).read(3).encoding.to_s }
show("reader read(n)") do
  r = Zlib::GzipReader.new(StringIO.new(GZ))
  [r.read(3), r.read(3), r.read(100), r.read(3), r.read]
end
show("reader gets") do
  r = Zlib::GzipReader.new(StringIO.new(GZ))
  [r.gets, r.lineno, r.gets, r.gets]
end
show("reader gets with a separator") do
  r = Zlib::GzipReader.new(StringIO.new(GZ))
  [r.gets("o"), r.gets(nil)]
end
show("reader readlines") { Zlib::GzipReader.new(StringIO.new(GZ)).readlines }
show("reader each_line") { Zlib::GzipReader.new(StringIO.new(GZ)).each_line.to_a }
show("reader each is Enumerable's") { Zlib::GzipReader.new(StringIO.new(GZ)).map(&:chomp) }
show("reader chars and bytes") do
  r = Zlib::GzipReader.new(StringIO.new(GZ))
  [r.getc, r.getbyte, r.readchar, r.readbyte]
end
show("reader each_byte") { Zlib::GzipReader.new(StringIO.new(Zlib.gzip("ab"))).each_byte.to_a }
show("reader each_char") { Zlib::GzipReader.new(StringIO.new(Zlib.gzip("ab"))).each_char.to_a }
show("reader ungetc") do
  r = Zlib::GzipReader.new(StringIO.new(GZ))
  c = r.getc
  r.ungetc(c)
  r.gets
end
show("reader readpartial") { Zlib::GzipReader.new(StringIO.new(GZ)).readpartial(4) }
show("reader eof and pos") do
  r = Zlib::GzipReader.new(StringIO.new(GZ))
  before = r.eof?
  r.read
  [before, r.pos, r.eof?, r.eof]
end
show("reader readline past the end") do
  r = Zlib::GzipReader.new(StringIO.new(GZ))
  r.read
  r.readline
end
show("reader rewind") do
  r = Zlib::GzipReader.new(StringIO.new(GZ))
  r.read
  r.rewind
  r.gets
end
show("reader unused") do
  r = Zlib::GzipReader.new(StringIO.new(GZ + "EXTRA"))
  r.read
  r.unused
end
show("reader external_encoding") { Zlib::GzipReader.new(StringIO.new(GZ)).external_encoding.to_s }
show("reader wrap") { Zlib::GzipReader.wrap(StringIO.new(GZ)) { |r| r.read } }
show("reader zcat") { Zlib::GzipReader.zcat(StringIO.new(GZ + GZ)) }
show("reader crc after reading") do
  r = Zlib::GzipReader.new(StringIO.new(GZ))
  r.read
  r.crc == Zlib.crc32("hello\nworld\n")
end

# A file on disk, through both halves.
show("open round-trip") do
  Zlib::GzipWriter.open("zlib_classes_tmp.gz") { |w| w.write("through a file") }
  Zlib::GzipReader.open("zlib_classes_tmp.gz") { |r| r.read }
ensure
  File.unlink("zlib_classes_tmp.gz")
end

# Corruption. CRuby checks a member's footer only once the buffer it filled
# has been fully handed out, so `read` (which answers everything at once)
# reports nothing and the line-at-a-time readers do -- reproduced faithfully.
def corrupt(offset)
  bad = GZ.dup
  bad.setbyte(bad.bytesize + offset, GZ.getbyte(GZ.bytesize + offset) ^ 0xff)
  bad
end
show("not gzip at all") { Zlib::GzipReader.new(StringIO.new("not gzip at all")) }
show("bad crc via read") { Zlib::GzipReader.new(StringIO.new(corrupt(-8))).read }
show("bad crc via readlines") { Zlib::GzipReader.new(StringIO.new(corrupt(-8))).readlines }
show("bad length via gunzip") { Zlib.gunzip(corrupt(-4)) }
show("bad crc via gunzip") { Zlib.gunzip(corrupt(-8)) }
show("truncated member") { Zlib::GzipReader.new(StringIO.new(GZ[0..15])).read }

# -- Checksums --------------------------------------------------------------

show("crc32") { [Zlib.crc32("abc"), Zlib.crc32, Zlib.crc32("abc", 100)] }
show("adler32") { [Zlib.adler32("abc"), Zlib.adler32] }
show("crc32 continued") { Zlib.crc32("def", Zlib.crc32("abc")) == Zlib.crc32("abcdef") }
