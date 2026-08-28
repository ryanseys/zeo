# Indexing a string that holds only 7-bit bytes is byte indexing: #length is
# the byte length and s[i] is the byte at i. spinel had folded the UTF-8 walk
# into a pointer-keyed length cache, so the walk was already gone -- but every
# index still PROBED that cache twice (once for #length, once for the byte
# offset), about 160 instructions per index. A bit in the string header answers
# the same question with two loads and cannot be evicted.
#
# The bit says nothing about encoding. US-ASCII and UTF-8 are compatible and
# spinel does not distinguish them (docs/limitations.md); this is a fact about
# the BYTES, verified by a scan that was happening anyway and cleared wherever
# the bytes can change.
#
# What this program checks is the clearing. A string measured while it is
# 7-bit, then given a byte that is not, has to answer the new truth.

s = String.new("abc")
p s.length
p s[1]
s << "é"
p s.length
p s[3]
p s.chars
p s.bytesize

# in-place, same length
t = String.new("abcdef")
p t.length
t[2] = "é"
p t.length
p t[2]

# setbyte: no length change at all, so only the content invariant catches it
y = String.new("abc")
p y.length
y.setbyte(1, 233)
p y.bytesize
p y.bytes

# replace
u = String.new("abc")
p u.length
u.replace("éé")
p u.length
p u[0]

# a grow that reallocates the buffer
g = String.new("abc")
p g.length
40.times { g << "x" }
g << "é"
p g.length
p g[-1]

# through the shared-mutable handle a block capture turns it into
buf = String.new("abcd")
p buf.length
pr = proc { buf << "é" }
pr.call
p buf.length
p buf[4]
p buf.chars.length

# a BINARY string keeps its own path: the two tags must not be confused
b = [200, 65].pack("C*")
p b.length
p b.encoding.to_s
p b.bytes
p b.b.length
p b[0].bytes

# slices are measured in their own right
z = String.new("hello world")
p z.length
p z[0, 5].length
p z[6, 5][1]

# and a genuinely multi-byte string is never flagged
m = "日本語"
p m.length
p m.bytesize
p m[1]
p m.chars
m2 = String.new("日本語")
m2 << "x"
p m2.length
p m2[3]

# mixed, measured before and after
x = String.new("abc")
p x.length
x << "日"
p x.length
p x[3]
x << "d"
p x.length
p x[4]
