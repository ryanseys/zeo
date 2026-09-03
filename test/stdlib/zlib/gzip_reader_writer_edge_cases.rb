# The gzip container's edge surface: line-reading modes, limits, buffers,
# unget, multi-member streams, and the exact error classes with their
# `input` payloads. Every row is deterministic -- no timestamps, no
# compressed bytes printed, only decoded data and error shapes.
require "zlib"
require "stringio"

def show(label)
  print "#{label}: "
  p(yield)
rescue => e
  puts "#{e.class}: #{e.message}"
end

MB = Zlib.gzip("héllo\nwörld\n")
PARA = Zlib.gzip("aaa\n\n\n\nbbb\nccc\n\nddd")

# -- gets: separators, paragraph mode, limits -------------------------------

show("paragraph gets") do
  r = Zlib::GzipReader.new(StringIO.new(PARA))
  [r.gets(""), r.gets(""), r.gets(""), r.gets("")]
end
show("gets limit never splits a character") do
  r = Zlib::GzipReader.new(StringIO.new(MB))
  [r.gets(2), r.gets(1), r.gets]
end
show("gets limit 0") { Zlib::GzipReader.new(StringIO.new(MB)).gets(0) }
show("gets sep and limit") { Zlib::GzipReader.new(StringIO.new(Zlib.gzip("axbxc"))).gets("x", 10) }
show("gets rejects a hash") { Zlib::GzipReader.new(StringIO.new(MB)).gets(chomp: true) }
show("gets encoding") { Zlib::GzipReader.new(StringIO.new(MB)).gets.encoding.to_s }
show("gets at eof keeps lineno") do
  r = Zlib::GzipReader.new(StringIO.new(MB))
  r.read
  [r.gets, r.lineno]
end
show("readlines with a separator") { Zlib::GzipReader.new(StringIO.new(Zlib.gzip("axbxc"))).readlines("x") }
show("readlines with a limit") { Zlib::GzipReader.new(StringIO.new(MB)).readlines(4) }
# The blockless enumerator drops the arguments; the block form honours them.
show("blockless each_line drops its arguments") { Zlib::GzipReader.new(StringIO.new(MB)).each_line(3).to_a }
show("each_line block honours the limit") do
  lines = []
  Zlib::GzipReader.new(StringIO.new(MB)).each_line(3) { |l| lines << l }
  lines
end
show("lineno=") do
  r = Zlib::GzipReader.new(StringIO.new(MB))
  r.lineno = 5
  r.gets
  r.lineno
end

# -- read: buffers, lengths -------------------------------------------------

show("read into a buffer keeps its encoding") do
  buf = +"seed"
  r = Zlib::GzipReader.new(StringIO.new(MB))
  got = r.read(3, buf)
  [got.equal?(buf), buf, buf.encoding.to_s]
end
show("whole-member read ignores the buffer") do
  buf = +"seed"
  r = Zlib::GzipReader.new(StringIO.new(MB))
  got = r.read(nil, buf)
  [got.equal?(buf), buf]
end
show("readpartial into a buffer") do
  buf = +"s"
  Zlib::GzipReader.new(StringIO.new(MB)).readpartial(4, buf)
  [buf, buf.encoding.to_s]
end
show("negative read") { Zlib::GzipReader.new(StringIO.new(MB)).read(-1) }
show("empty member") do
  r = Zlib::GzipReader.new(StringIO.new(Zlib.gzip("")))
  [r.eof?, r.read, r.read(3)]
end
show("getc completes a multibyte character") do
  r = Zlib::GzipReader.new(StringIO.new(MB))
  [r.getc, r.getc]
end

# -- unget and pos ----------------------------------------------------------

show("pos follows ungetc backwards") do
  r = Zlib::GzipReader.new(StringIO.new(MB))
  a = r.read(3)
  p1 = r.pos
  r.ungetc("x")
  p2 = r.pos
  [a, p1, p2, r.read(2), r.pos]
end
show("ungetc accepts a codepoint") do
  r = Zlib::GzipReader.new(StringIO.new(MB))
  r.ungetc(65)
  r.read(3)
end
show("ungetbyte takes one byte of a string") do
  r = Zlib::GzipReader.new(StringIO.new(MB))
  r.ungetbyte("AB")
  r.read(4)
end
show("ungetbyte wraps an integer") do
  r = Zlib::GzipReader.new(StringIO.new(MB))
  r.ungetbyte(300)
  r.read(2)
end

# -- zcat -------------------------------------------------------------------

show("zcat with a block yields each member") do
  members = []
  answer = Zlib::GzipReader.zcat(StringIO.new(Zlib.gzip("one") + Zlib.gzip("two"))) { |d| members << d }
  [members, answer]
end
show("zcat result encoding") { Zlib::GzipReader.zcat(StringIO.new(Zlib.gzip("x"))).encoding.to_s }
show("zcat refuses trailing garbage") { Zlib::GzipReader.zcat(StringIO.new(Zlib.gzip("one") + "junk")) }

# -- the error shapes -------------------------------------------------------

show("a header-phase error carries the input") do
  Zlib::GzipReader.new(StringIO.new("junkdata"))
rescue Zlib::GzipFile::Error => e
  [e.message, e.input]
end
show("an EOF inside the ten header bytes is not gzip") do
  Zlib::GzipReader.new(StringIO.new("\x1f\x8b\x08"))
rescue Zlib::GzipFile::Error => e
  [e.message, e.input]
end
show("an EOF past them is a truncated member") do
  Zlib::GzipReader.new(StringIO.new("\x1f\x8b\x08\x08\x00\x00\x00\x00\x00\x03na"))
rescue Zlib::GzipFile::Error => e
  [e.message, e.input]
end
show("a crc error carries no input") do
  gz = Zlib.gzip("hello") + "TAIL"
  gz.setbyte(gz.bytesize - 12, gz.getbyte(gz.bytesize - 12) ^ 0xff)
  Zlib::GzipReader.new(StringIO.new(gz)).readlines
rescue Zlib::GzipFile::CRCError => e
  [e.message, e.input]
end
show("a missing footer carries no input") do
  gz = Zlib.gzip("hello")
  Zlib::GzipReader.new(StringIO.new(gz[0..-9])).readlines
rescue Zlib::GzipFile::NoFooter => e
  [e.message, e.input]
end
show("GzipFile itself does not construct") { Zlib::GzipFile.new }
show("a dup arrives closed") do
  Zlib::GzipReader.new(StringIO.new(MB)).dup.read
end

# -- the writer's remaining edges -------------------------------------------

show("write takes several arguments") do
  io = StringIO.new(+"".b)
  w = Zlib::GzipWriter.new(io)
  n = w.write("ab", "cd")
  w.close
  [n, Zlib.gunzip(io.string)]
end
show("puts flattens arrays and drops empty ones") do
  io = StringIO.new(+"".b)
  w = Zlib::GzipWriter.new(io)
  w.puts(["a", ["b", []]])
  w.close
  Zlib.gunzip(io.string)
end
show("putc writes one byte") do
  io = StringIO.new(+"".b)
  w = Zlib::GzipWriter.new(io)
  w.putc("éx")
  w.putc(233)
  w.close
  Zlib.gunzip(io.string).bytes
end
show("mtime= accepts a Time") do
  io = StringIO.new(+"".b)
  w = Zlib::GzipWriter.new(io)
  w.mtime = Time.at(99)
  w.write("x")
  w.close
  Zlib::GzipReader.new(StringIO.new(io.string)).mtime.to_i
end
show("flush answers the writer") do
  io = StringIO.new(+"".b)
  w = Zlib::GzipWriter.new(io)
  answered = w.flush.equal?(w)
  w.close
  answered
end
show("sync flag round-trips") do
  io = StringIO.new(+"".b)
  w = Zlib::GzipWriter.new(io)
  before = w.sync
  w.sync = true
  read_back = w.sync
  w.close
  [before, read_back]
end

# -- the stream counter the layer leans on ----------------------------------

show("a finished raw inflate counts only consumed input") do
  raw = Zlib::Deflate.new(nil, -Zlib::MAX_WBITS).deflate("hello", Zlib::FINISH)
  z = Zlib::Inflate.new(-Zlib::MAX_WBITS)
  z.inflate(raw + "XYZW")
  [z.total_in, raw.bytesize]
end
__END__
paragraph gets: ["aaa\n\n", "bbb\nccc\n\n", "ddd", nil]
gets limit never splits a character: ["hé", "l", "lo\n"]
gets limit 0: ""
gets sep and limit: "ax"
gets rejects a hash: TypeError: no implicit conversion of Hash into Integer
gets encoding: "UTF-8"
gets at eof keeps lineno: [nil, 0]
readlines with a separator: ["ax", "bx", "c"]
readlines with a limit: ["hél", "lo\n", "wör", "ld\n"]
blockless each_line drops its arguments: ["héllo\n", "wörld\n"]
each_line block honours the limit: ["hé", "llo", "\n", "wö", "rld", "\n"]
lineno=: 6
read into a buffer keeps its encoding: [true, "hé", "UTF-8"]
whole-member read ignores the buffer: [false, "seed"]
readpartial into a buffer: ["hél", "UTF-8"]
negative read: ArgumentError: negative length -1 given
empty member: [true, "", nil]
getc completes a multibyte character: ["h", "é"]
pos follows ungetc backwards: ["h\xC3\xA9", 3, 2, "xl", 4]
ungetc accepts a codepoint: "Ah\xC3"
ungetbyte takes one byte of a string: "Ah\xC3\xA9"
ungetbyte wraps an integer: ",h"
zcat with a block yields each member: [["one", "two"], nil]
zcat result encoding: "ASCII-8BIT"
zcat refuses trailing garbage: Zlib::GzipFile::Error: not in gzip format
a header-phase error carries the input: ["not in gzip format", "junkdata"]
an EOF inside the ten header bytes is not gzip: ["not in gzip format", "\x1F\x8B\b"]
an EOF past them is a truncated member: ["unexpected end of file", nil]
a crc error carries no input: ["invalid compressed data -- crc error", nil]
a missing footer carries no input: ["footer is not found", nil]
GzipFile itself does not construct: TypeError: allocator undefined for Zlib::GzipFile
a dup arrives closed: Zlib::GzipFile::Error: closed gzip stream
write takes several arguments: [4, "abcd"]
puts flattens arrays and drops empty ones: "a\nb\n"
putc writes one byte: [195, 233]
mtime= accepts a Time: 99
flush answers the writer: true
sync flag round-trips: [false, true]
a finished raw inflate counts only consumed input: [7, 7]
