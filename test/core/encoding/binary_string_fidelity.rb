# Binary strings must survive every operation that copies or moves them.
#
# This is a REGRESSION SUITE for one recurring defect, not a feature test.
# Several methods used to rebuild a String from its "display text" -- decoding
# the bytes as UTF-8 and re-encoding them -- which silently rewrites any byte
# that isn't valid UTF-8. It corrupts both ways:
#
#   0x8b  ->  0xEF 0xBF 0xBD   (lossy decode: replaced with U+FFFD)
#   0x8b  ->  0xC2 0x8B        (Latin-1 promotion: re-encoded, length changed)
#
# It has been found and fixed in `String#+`, `Array#join`, `String#dup`,
# `StringIO`, and `IO#read` at different times. Each fix without a test let the
# next one through, so every operation that touches bytes is pinned here.
#
# The bytes below are a real gzip header: 0x8b and 0xc8 are both invalid UTF-8
# on their own, which is what makes them the case that catches this.
require "tmpdir"
ZTMP = Dir.mktmpdir

RAW = [0x1f, 0x8b, 0x08, 0x00, 0xc8].pack("C*")

def show(label)
  print "#{label}: "
  p(yield)
rescue => e
  puts "#{e.class}: #{e.message}"
end

show("pack") { [RAW.bytes, RAW.encoding.to_s, RAW.bytesize] }

# Copying.
show("dup") { RAW.dup.bytes }
show("clone") { RAW.clone.bytes }
show("dup encoding") { RAW.dup.encoding.to_s }
show("+@") { (+RAW).bytes }

# Concatenation and splitting.
show("plus") { (RAW[0, 2] + RAW[2..]).bytes }
show("join") { [RAW[0, 2], RAW[2..]].join.bytes }
show("join nested") { [[RAW[0, 2]], [RAW[2..]]].join.bytes }
show("join separator") { [RAW[0, 1], RAW[1, 1]].join(RAW[4, 1]).bytes }
show("slice") { RAW[1, 3].bytes }
show("each_byte") { RAW.each_byte.to_a }
show("bytes reverse") { RAW.reverse.bytes }
show("append") { (+"").tap { |s| s << RAW }.bytes }
show("multiply") { (RAW[1, 1] * 3).bytes }

# In-place byte edits.
show("setbyte") { RAW.dup.tap { |s| s.setbyte(4, 0x37) }.bytes }
show("getbyte") { [RAW.getbyte(1), RAW.getbyte(4)] }

# Arrays and hashes carrying binary values.
show("array round-trip") { [RAW].first.bytes }
show("hash value") { { k: RAW }[:k].bytes }
show("array pack/unpack") { RAW.unpack("C*").pack("C*").bytes }

# Files, both APIs.
show("binwrite/binread") do
  File.binwrite(File.join(ZTMP, "binary_fidelity_tmp.bin"), RAW)
  File.binread(File.join(ZTMP, "binary_fidelity_tmp.bin")).bytes
end
show("File#write then #read") do
  File.open(File.join(ZTMP, "binary_fidelity_tmp.bin"), "wb") { |f| f.write(RAW) }
  File.open(File.join(ZTMP, "binary_fidelity_tmp.bin"), "rb") { |f| f.read.bytes }
end
show("File#read(n)") do
  File.open(File.join(ZTMP, "binary_fidelity_tmp.bin"), "rb") { |f| f.read(3).bytes }
end
show("File#read(n) encoding") do
  File.open(File.join(ZTMP, "binary_fidelity_tmp.bin"), "rb") { |f| f.read(3).encoding.to_s }
ensure
  File.unlink(File.join(ZTMP, "binary_fidelity_tmp.bin"))
end

# StringIO, which is a byte buffer that has to remember what its bytes mean.
require "stringio"
show("StringIO string") { StringIO.new(RAW).string.bytes }
show("StringIO read") { StringIO.new(RAW).read.bytes }
show("StringIO read enc") { StringIO.new(RAW).read.encoding.to_s }
show("StringIO read(n)") { StringIO.new(RAW).read(3).bytes }
show("StringIO read(n) enc") { StringIO.new(RAW).read(3).encoding.to_s }
show("StringIO getc") { StringIO.new(RAW).getc.bytes }
show("StringIO write") { StringIO.new("".b).tap { |io| io.write(RAW) }.string.bytes }
show("StringIO gets") { StringIO.new(RAW + "\n".b).gets.bytes }
show("StringIO readlines") { StringIO.new(RAW).readlines.map(&:bytes) }

# A UTF-8 buffer must keep ITS encoding and its character boundaries -- the
# fixes above must not have turned everything into bytes.
show("utf8 StringIO getc") { StringIO.new("éa").getc }
show("utf8 StringIO read enc") { StringIO.new("éa").read.encoding.to_s }
show("utf8 dup") { "é".dup.encoding.to_s }
show("utf8 join") { ["é", "a"].join }
__END__
pack: [[31, 139, 8, 0, 200], "ASCII-8BIT", 5]
dup: [31, 139, 8, 0, 200]
clone: [31, 139, 8, 0, 200]
dup encoding: "ASCII-8BIT"
+@: [31, 139, 8, 0, 200]
plus: [31, 139, 8, 0, 200]
join: [31, 139, 8, 0, 200]
join nested: [31, 139, 8, 0, 200]
join separator: [31, 200, 139]
slice: [139, 8, 0]
each_byte: [31, 139, 8, 0, 200]
bytes reverse: [200, 0, 8, 139, 31]
append: [31, 139, 8, 0, 200]
multiply: [139, 139, 139]
setbyte: [31, 139, 8, 0, 55]
getbyte: [139, 200]
array round-trip: [31, 139, 8, 0, 200]
hash value: [31, 139, 8, 0, 200]
array pack/unpack: [31, 139, 8, 0, 200]
binwrite/binread: [31, 139, 8, 0, 200]
File#write then #read: [31, 139, 8, 0, 200]
File#read(n): [31, 139, 8]
File#read(n) encoding: "ASCII-8BIT"
StringIO string: [31, 139, 8, 0, 200]
StringIO read: [31, 139, 8, 0, 200]
StringIO read enc: "ASCII-8BIT"
StringIO read(n): [31, 139, 8]
StringIO read(n) enc: "ASCII-8BIT"
StringIO getc: [31]
StringIO write: [31, 139, 8, 0, 200]
StringIO gets: [31, 139, 8, 0, 200, 10]
StringIO readlines: [[31, 139, 8, 0, 200]]
utf8 StringIO getc: "é"
utf8 StringIO read enc: "UTF-8"
utf8 dup: "UTF-8"
utf8 join: "éa"
