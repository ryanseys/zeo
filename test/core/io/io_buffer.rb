# IO::Buffer: typed value access, slices over shared backing, masks, file
# mapping, and direct IO transfer. Addresses are scrubbed (the one
# nondeterminism); PAGE_SIZE is platform-dependent so only its shape prints.
require "tmpdir"
ZTMP = Dir.mktmpdir

$stderr.reopen(IO::NULL)
SCRUB = ->(s) { s.gsub(/0x\h{9,}/, "0xN") }

# ---- constants and the error tree.
p IO::Buffer::PAGE_SIZE >= 4096
p IO::Buffer::PAGE_SIZE % 4096 == 0
p IO::Buffer::DEFAULT_SIZE
p [IO::Buffer::EXTERNAL, IO::Buffer::INTERNAL, IO::Buffer::MAPPED, IO::Buffer::SHARED,
   IO::Buffer::LOCKED, IO::Buffer::PRIVATE, IO::Buffer::READONLY]
p [IO::Buffer::LITTLE_ENDIAN, IO::Buffer::BIG_ENDIAN, IO::Buffer::NETWORK_ENDIAN]
p IO::Buffer::HOST_ENDIAN == IO::Buffer::LITTLE_ENDIAN
p [IO::Buffer::LockedError.superclass, IO::Buffer::AllocationError.superclass,
   IO::Buffer::AccessError.superclass, IO::Buffer::InvalidatedError.superclass,
   IO::Buffer::MaskError.superclass]
p IO::Buffer.ancestors[0..2]

# ---- construction.
p IO::Buffer.new(16).size
p IO::Buffer.new(0).size
begin
  IO::Buffer.new(-1)
rescue ArgumentError => e
  p [e.class, e.message]
end
p [IO::Buffer.size_of(:U32), IO::Buffer.size_of([:U32, :U8]), IO::Buffer.size_of(:f64)]
begin
  IO::Buffer.size_of(:bogus)
rescue ArgumentError => e
  p [e.class, e.message]
end
p IO::Buffer.string(5) { |b| b.set_string("abcde") }

# ---- predicates.
b = IO::Buffer.new(16)
p [b.valid?, b.null?, b.empty?, b.external?, b.internal?, b.mapped?, b.shared?,
   b.locked?, b.readonly?, b.private?]
p [IO::Buffer.new(0).null?, IO::Buffer.new(0).empty?]

# ---- typed values.
b.set_value(:U32, 0, 0xdeadbeef)
p [b.get_value(:U32, 0), b.get_value(:u32, 0)]
p b.set_value(:U8, 0, 256)
p b.set_value(:U8, 1, -1)
p b.get_values([:U8, :U8], 0)
p b.set_values([:U8, :u16], 4, [7, 513])
p b.get_values([:U8, :u16], 4)
w = IO::Buffer.new(16)
w.set_value(:u64, 0, 2**63 + 5)
p [w.get_value(:u64, 0), w.get_value(:s64, 0)]
w.set_value(:U128, 0, 2**100)
p w.get_value(:U128, 0)
w.set_value(:f32, 0, 1.5)
p w.get_value(:f32, 0)
w.set_value(:F64, 8, -2.25)
p w.get_value(:F64, 8)
begin
  b.get_value(:U32, 14)
rescue ArgumentError => e
  p [e.class, e.message]
end

# ---- strings.
s = IO::Buffer.new(8)
p s.set_string("hi")
p s.get_string
p s.get_string(0, 2, Encoding::UTF_8).encoding
begin
  s.set_string("way too long to fit here")
rescue ArgumentError => e
  p [e.class, e.message]
end

# ---- slices share the backing.
sl = b.slice(4, 8)
p sl.size
sl.set_value(:U8, 0, 42)
p b.get_value(:U8, 4)
p [b.slice.size, b.slice(8).size]
begin
  b.slice(12, 8)
rescue ArgumentError => e
  p [e.class, e.message]
end
begin
  b.slice(-1, 2)
rescue ArgumentError => e
  p [e.class, e.message]
end
r = IO::Buffer.new(8)
r.set_value(:U8, 0, 7)
rs = r.slice(0, 4)
r.resize(16)
p [r.size, r.get_value(:U8, 0), rs.get_value(:U8, 0), rs.valid?]

# ---- free / null / transfer.
f = IO::Buffer.new(8)
f.free
p [f.valid?, f.null?, f.size]
t = IO::Buffer.new(8)
t.set_value(:U8, 0, 9)
t2 = t.transfer
p [t2.get_value(:U8, 0), t.null?, t.valid?]

# ---- locked.
l = IO::Buffer.new(8)
p l.locked { |x| [x.equal?(l), l.locked?] }
p l.locked?
begin
  l.locked { l.locked { } }
rescue IO::Buffer::LockedError => e
  p [e.class, e.message]
end
begin
  l.locked { l.resize(16) }
rescue IO::Buffer::LockedError => e
  p [e.class, e.message]
end
begin
  l.locked { l.free }
rescue IO::Buffer::LockedError => e
  p [e.class, e.message]
end

# ---- readonly.
ro = IO::Buffer.for("readonly str")
p [ro.size, ro.readonly?]
p ro.get_string(0, 8)
begin
  ro.set_value(:U8, 0, 1)
rescue IO::Buffer::AccessError => e
  p [e.class, e.message]
end
begin
  ro.clear
rescue IO::Buffer::AccessError => e
  p [e.class, e.message]
end

# ---- copy.
a = IO::Buffer.new(8)
src = IO::Buffer.for("abcdefgh").dup
p a.copy(src)
p a.get_string
p a.copy(src, 4, 2)
p a.get_string
p a.copy(src, 0, 4, 4)
p a.get_string
begin
  a.copy(src, 6, 4)
rescue ArgumentError => e
  p [e.class, e.message]
end

# ---- comparison.
c1 = IO::Buffer.for("abc").dup
c2 = IO::Buffer.for("abd").dup
c3 = IO::Buffer.for("abc").dup
p [c1 <=> c2, c2 <=> c1, c1 <=> c3]
p [c1 == c3, c1 == c2]
p [c1 <=> IO::Buffer.for("ab").dup, IO::Buffer.for("ab").dup <=> c1]
p [c1 < c2, c1 >= c3, c1.between?(c3, c2)]
begin
  c1 <=> "abc"
rescue TypeError => e
  p [e.class, e.message]
end

# ---- bitwise + masks.
m = IO::Buffer.for("\x0f\xf0\xff\x00").dup
k = IO::Buffer.for("\xff\xff\x0f\x0f").dup
p (m & k).get_string.bytes
p (m | k).get_string.bytes
p (m ^ k).get_string.bytes
p (~m).get_string.bytes
p (m & IO::Buffer.for("\x01\x02").dup).get_string.bytes
mm = IO::Buffer.for("\x0f\xf0").dup
mm.and!(IO::Buffer.for("\xff\x00").dup)
p mm.get_string.bytes
mm.or!(IO::Buffer.for("\x00\x01").dup)
p mm.get_string.bytes
mm.xor!(IO::Buffer.for("\xff\xff").dup)
p mm.get_string.bytes
mm.not!
p mm.get_string.bytes
mm.and!(IO::Buffer.for("\x01").dup)
p mm.get_string.bytes
begin
  mm & IO::Buffer.new(0)
rescue IO::Buffer::MaskError => e
  p [e.class, e.message]
end
begin
  mm.and!(IO::Buffer.new(0))
rescue IO::Buffer::MaskError => e
  p [e.class, e.message]
end

# ---- iteration.
e = IO::Buffer.for("\x01\x02\x03\x04").dup
acc = []
e.each(:U8) { |off, v| acc << [off, v] }
p acc
acc = []
e.each(:u16) { |off, v| acc << [off, v] }
p acc
acc = []
e.each(:U8, 1, 2) { |off, v| acc << [off, v] }
p acc
p e.each(:U8).class
p e.values(:U8)
p e.values(:u16, 2)
acc = []
e.each_byte { |x| acc << x }
p acc
p e.each_byte.to_a

# ---- clear.
p IO::Buffer.for("abcdefgh").dup.clear(0x2a).get_string
p IO::Buffer.for("abcdefgh").dup.clear(0, 2).get_string
p IO::Buffer.for("abcdefgh").dup.clear(9, 2, 4).get_string
p IO::Buffer.for("ab").dup.clear.get_string.bytes

# ---- hexdump and inspect.
h = IO::Buffer.for("0123456789abcdefghij").dup
puts h.hexdump
puts h.hexdump(4)
puts h.hexdump(4, 8)
puts h.hexdump(0, 20, 8)
p SCRUB.(IO::Buffer.new(4).inspect)
p SCRUB.(IO::Buffer.new(4).slice(1, 2).inspect)
p SCRUB.(IO::Buffer.for("literal").inspect)
p SCRUB.(IO::Buffer.for("literal").dup.inspect)
fz = IO::Buffer.new(4)
fz.free
p SCRUB.(fz.inspect)
p SCRUB.(IO::Buffer.new(4).to_s)
lk = IO::Buffer.new(4)
lk.locked { p SCRUB.(lk.inspect.lines.first) }
big = IO::Buffer.new(300)
p big.inspect.lines.length
p big.inspect.lines.last

# ---- direct IO.
path = File.join(ZTMP, "zeo_io_buffer_fixture.txt")
File.write(path, "ABCDEFGHIJKLMNOPQR")
fh = File.open(path)
rb = IO::Buffer.new(10)
p rb.read(fh)
p rb.get_string
rb2 = IO::Buffer.new(10)
rb2.clear(0x2e)
p rb2.pread(fh, 3, 4, 6)
p rb2.get_string
wpath = File.join(ZTMP, "zeo_io_buffer_fixture_w.txt")
File.write(wpath, "")
wh = File.open(wpath, "r+")
wsrc = IO::Buffer.for("0123456789").dup
p wsrc.write(wh)
p wsrc.write(wh, 3, 7)
wh.flush
p File.read(wpath)
p wsrc.pwrite(wh, 0, 3, 5)
wh.flush
p File.read(wpath)
closed = File.open(path)
closed.close
begin
  rb.read(closed)
rescue IOError => e
  p [e.class, e.message]
end

# ---- map.
mp = IO::Buffer.map(File.open(path, "r"), nil, 0, IO::Buffer::READONLY)
p [mp.size, mp.readonly?, mp.mapped?, mp.get_string(0, 4)]
p SCRUB.(mp.inspect.lines.first)
rw = IO::Buffer.map(File.open(path, "r+"))
p [rw.size, rw.readonly?]
rw.set_string("ZZ")
p File.read(path)[0, 4]
p IO::Buffer.map(File.open(path, "r+"), nil, 0, IO::Buffer::PRIVATE).private?

# ---- for block form: writable view, write-back at exit.
mut = +"mutable str"
res = IO::Buffer.for(mut) { |bb| p [bb.readonly?, bb.size, bb.external?]; bb.set_string("X"); :blockresult }
p res
p mut

# ---- dup and initialize_copy.
d = IO::Buffer.for("dup me").dup
p [d.readonly?, d.internal?, d.get_string]
orig = IO::Buffer.new(4)
orig.set_value(:U8, 0, 5)
cp = orig.dup
cp.set_value(:U8, 0, 9)
p [orig.get_value(:U8, 0), cp.get_value(:U8, 0)]
__END__
true
true
65536
[1, 2, 4, 8, 32, 64, 128]
[4, 8, 8]
true
[RuntimeError, RuntimeError, RuntimeError, RuntimeError, ArgumentError]
[IO::Buffer, Comparable, Object]
16
0
[ArgumentError, "Size can't be negative!"]
[4, 5, 8]
[ArgumentError, "Invalid type name!"]
"abcde"
[true, false, false, false, true, false, false, false, false, false]
[true, true]
[3735928559, 4022250974]
1
2
[0, 255]
7
[7, 513]
[9223372036854775813, -9223372036854775803]
1267650600228229401496703205376
1.5
-2.25
[ArgumentError, "Type extends beyond end of buffer! (offset=14 > size=16)"]
2
"hi\x00\x00\x00\x00\x00\x00"
#<Encoding:UTF-8>
[ArgumentError, "Specified offset+length is bigger than the buffer size!"]
8
42
[16, 8]
[ArgumentError, "Specified offset+length is bigger than the buffer size!"]
[ArgumentError, "Offset can't be negative!"]
[16, 7, 7, true]
[true, true, 0]
[9, true, true]
[true, true]
false
[IO::Buffer::LockedError, "Buffer already locked!"]
[IO::Buffer::LockedError, "Cannot resize locked buffer!"]
[IO::Buffer::LockedError, "Buffer is locked!"]
[12, true]
"readonly"
[IO::Buffer::AccessError, "Buffer is not writable!"]
[IO::Buffer::AccessError, "Buffer is not writable!"]
8
"abcdefgh"
2
"abcdabgh"
4
"efghabgh"
[ArgumentError, "Specified offset+length is bigger than the buffer size!"]
[-1, 1, 0]
[true, false]
[1, -1]
[true, true, true]
[TypeError, "wrong argument type String (expected IO::Buffer)"]
[15, 240, 15, 0]
[255, 255, 255, 15]
[240, 15, 240, 15]
[240, 15, 0, 255]
[1, 0, 1, 0]
[15, 0]
[15, 1]
[240, 254]
[15, 1]
[1, 1]
[IO::Buffer::MaskError, "Zero-length mask given!"]
[IO::Buffer::MaskError, "Zero-length mask given!"]
[[0, 1], [1, 2], [2, 3], [3, 4]]
[[0, 513], [2, 1027]]
[[1, 2], [2, 3]]
Enumerator
[1, 2, 3, 4]
[1027]
[1, 2, 3, 4]
[1, 2, 3, 4]
"********"
"ab\x00\x00\x00\x00\x00\x00"
"ab\t\t\t\tgh"
[0, 0]
0xADDR  30 31 32 33 34 35 36 37 38 39 61 62 63 64 65 66 0123456789abcdef
0xADDR  67 68 69 6a                                     ghij
0xADDR  34 35 36 37 38 39 61 62 63 64 65 66 67 68 69 6a 456789abcdefghij
0xADDR  34 35 36 37 38 39 61 62                         456789ab
0xADDR  30 31 32 33 34 35 36 37 01234567
0xADDR  38 39 61 62 63 64 65 66 89abcdef
0xADDR  67 68 69 6a             ghij
"#<IO::Buffer 0xN+4 INTERNAL>\n0xADDR  00 00 00 00                                     ...."
"#<IO::Buffer 0xN+2 SLICE>\n0xADDR  00 00                                           .."
"#<IO::Buffer 0xN+7 EXTERNAL READONLY SLICE>\n0xADDR  6c 69 74 65 72 61 6c                            literal"
"#<IO::Buffer 0xN+7 INTERNAL>\n0xADDR  6c 69 74 65 72 61 6c                            literal"
"#<IO::Buffer 0xN+0 NULL>"
"#<IO::Buffer 0xN+4 INTERNAL>"
"#<IO::Buffer 0xN+4 INTERNAL LOCKED>\n"
18
"(and 44 more bytes not printed)"
10
"ABCDEFGHIJ"
4
"......DEFG"
10
3
"0123456789789"
5
"5678956789789"
[IOError, "closed stream"]
[18, true, true, "ABCD"]
"#<IO::Buffer 0xN+18 EXTERNAL MAPPED FILE SHARED READONLY>\n"
[18, false]
"ZZCD"
true
[false, 11, true]
:blockresult
"Xutable str"
[false, true, "dup me"]
[5, 9]
