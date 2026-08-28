# `IO.copy_stream`'s third and fourth arguments.
#
# Both were accepted and dropped. `copy_length` is what rubygems unpacks every
# `.gem` with -- `copy_stream(tar.io, out, entry.size)` -- so every extracted
# file got the rest of the archive appended to it. A pure-ruby gem survived
# that (ruby never reads past the code it needs); a C extension did not, and
# the compiler lexing megabytes of trailing NUL bytes is what made a
# `gem install` appear to hang.
#
# The negative cases are the surprising half and are the reason they are here:
# a negative length copies EVERYTHING and a negative offset reads from the
# start, neither of which raises.

require "stringio"
require "tempfile"
require "zlib"

BODY = (0...1000).map { |i| (97 + i % 26).chr }.join
DIR = Dir.mktmpdir("copy-stream")
SRC = File.join(DIR, "src.bin")
File.binwrite(SRC, BODY)
GZ = File.join(DIR, "src.gz")
Zlib::GzipWriter.open(GZ) { |w| w.write BODY }

def report(label)
  out = File.join(DIR, "out.bin")
  File.binwrite(out, "")
  n = yield out
  data = File.binread(out)
  puts format("%-26s ret=%-5s wrote=%-5d head=%s", label, n.inspect, data.bytesize, data[0, 6].inspect)
rescue StandardError => e
  puts format("%-26s %s: %s", label, e.class, e.message)
end

# --- a length, from a file ------------------------------------------------
report("no length") { |o| IO.copy_stream(SRC, o) }
report("length 300") { |o| IO.copy_stream(SRC, o, 300) }
report("length 0") { |o| IO.copy_stream(SRC, o, 0) }
report("length past the end") { |o| IO.copy_stream(SRC, o, 99_999) }
report("length -1") { |o| IO.copy_stream(SRC, o, -1) }
report("length 5.9") { |o| IO.copy_stream(SRC, o, 5.9) }
report("length a String") { |o| IO.copy_stream(SRC, o, "5") }

# --- an offset ------------------------------------------------------------
report("offset 200") { |o| IO.copy_stream(SRC, o, 10, 200) }
report("offset -1") { |o| IO.copy_stream(SRC, o, 10, -1) }
report("offset past the end") { |o| IO.copy_stream(SRC, o, 10, 99_999) }
report("offset, no length") { |o| IO.copy_stream(SRC, o, nil, 990) }

# --- an IO source, which advances -----------------------------------------
report("io, length 300") { |o| File.open(SRC, "rb") { |i| IO.copy_stream(i, o, 300) } }
report("io, two copies") do |o|
  File.open(SRC, "rb") { |i| IO.copy_stream(i, o, 10) + IO.copy_stream(i, o, 10) }
end
# An offset reads without moving the position -- the answer here is `pos`.
report("io, offset keeps pos") do |o|
  File.open(SRC, "rb") { |i| IO.copy_stream(i, o, 10, 200); i.pos }
end

# --- the sources rubygems actually unpacks through ------------------------
report("StringIO source") { |o| IO.copy_stream(StringIO.new(BODY), o, 300) }
report("gzip source") { |o| Zlib::GzipReader.open(GZ) { |i| IO.copy_stream(i, o, 300) } }
report("gzip, two copies") do |o|
  Zlib::GzipReader.open(GZ) { |i| IO.copy_stream(i, o, 10) + IO.copy_stream(i, o, 10) }
end
report("StringIO + offset") { |o| IO.copy_stream(StringIO.new(BODY), o, 10, 200) }

# --- a source shorter than the length asked for ---------------------------
SHORT = File.join(DIR, "short.bin")
File.binwrite(SHORT, "abc")
report("short source") { |o| IO.copy_stream(SHORT, o, 300) }
EMPTY = File.join(DIR, "empty.bin")
File.binwrite(EMPTY, "")
report("empty source") { |o| IO.copy_stream(EMPTY, o, 300) }

# --- an IO destination ----------------------------------------------------
report("io destination") { |o| File.open(o, "wb") { |w| IO.copy_stream(SRC, w, 300) } }
sink = StringIO.new(+"")
n = IO.copy_stream(StringIO.new(BODY), sink, 7)
puts format("%-26s ret=%-5s wrote=%-5d head=%s", "StringIO destination", n.inspect,
            sink.string.bytesize, sink.string[0, 6].inspect)

# The destination opens first, so a file copied onto itself is truncated
# before it is read.
SELF = File.join(DIR, "self.bin")
File.binwrite(SELF, BODY)
puts "onto itself: #{IO.copy_stream(SELF, SELF, 5).inspect} #{File.size(SELF)}"

# A missing source names the OPEN, not the copy.
begin
  IO.copy_stream(File.join(DIR, "absent"), File.join(DIR, "out2.bin"))
rescue SystemCallError => e
  puts "missing source: #{e.class} #{e.message.sub(DIR, '<dir>')}"
end
