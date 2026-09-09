# `#binmode` is not a no-op. The newline half of binary mode is one on
# Unix, which is presumably why this looked harmless -- but the ENCODING
# half is not: CRuby's `rb_io_binmode` and `strio_binmode` retag the stream
# ASCII-8BIT, and a caller binmodes a stream precisely so that nothing
# interprets the bytes.
#
# The second rule here is `external_encoding`'s: CRuby asks WRITABLE, not
# write-only, so a stream opened `"w+"` (every Tempfile) answers nil until
# something says what its bytes are to be read as.
require "tmpdir"
ZTMP = Dir.mktmpdir

require "stringio"
require "tempfile"
s = StringIO.new("héllo")
p [s.external_encoding.to_s, s.string.encoding.to_s]
s.binmode
p [s.external_encoding.to_s, s.read.encoding.to_s, s.string.encoding.to_s]
s.rewind
p s.read.bytes.length
w = StringIO.new(+"")
w.binmode
w.write("é")
p [w.string.encoding.to_s, w.string.bytesize]
f = Tempfile.new("zeo_binmode")
f.write("é")
f.flush
f.rewind
p [f.external_encoding.to_s, f.read.encoding.to_s]
f.rewind
f.binmode
p [f.external_encoding.to_s, f.internal_encoding.inspect, f.read.encoding.to_s, f.binmode?]
f.close!
g = File.open(File.join(ZTMP, "zeo_binmode_golden.bin"), "wb")
g.write("\xff\xfe".b)
g.close
p File.binread(File.join(ZTMP, "zeo_binmode_golden.bin")).encoding.to_s
h = File.open(File.join(ZTMP, "zeo_binmode_golden.bin"))
h.binmode
p [h.external_encoding.to_s, h.read.encoding.to_s]
h.close
File.delete(File.join(ZTMP, "zeo_binmode_golden.bin"))
__END__
["UTF-8", "UTF-8"]
["ASCII-8BIT", "ASCII-8BIT", "ASCII-8BIT"]
6
["ASCII-8BIT", 2]
["", "UTF-8"]
["ASCII-8BIT", "nil", "ASCII-8BIT", true]
"ASCII-8BIT"
["ASCII-8BIT", "ASCII-8BIT"]
