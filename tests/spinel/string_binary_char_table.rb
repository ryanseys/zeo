# `s[i]` on a BINARY string used to allocate: the shared 1-byte substring table
# is a static array whose entries carry no header (marker 0xff), and the
# ASCII-8BIT tag lives in a header -- so keeping the tag meant building a real
# string per index. That was 2.3x, and none of it was the indexing: 220M of the
# 226M difference was malloc, free, and the GC sweep walking what they left.
#
# A second static table carries a header, under a marker that says "static
# storage WITH a header" (0xfb). It joins the group that has a header and stays
# out of the group that is a mutable heap string, so nothing sweeps it, nothing
# mutates it in place, and its length is real -- which also puts byte 0 in the
# table, where strlen had made it the empty string.

b = [200, 65, 0, 255].pack("C*")
p b.bytesize
p b[0].bytes
p b[1].bytes
p b[2].bytes
p b[3].bytes
p b[0].encoding.to_s
p b[1].encoding.to_s
p b[0].length
p b[2].length
p b[2].bytesize
p b[0] == b[0]
p b[1] == "A"
p b[0].inspect
p b[2].inspect

# the slice is not frozen, and mutating a copy must not poison the shared entry
x = b[1]
p x.frozen?
y = x.dup
y << "z"
p y
p b[1].bytes
p b[1] == "A"

w2 = b[1].dup
w2.setbyte(0, 66)
p w2.bytes
p b[1].bytes

# as Hash keys
h = {}
h[b[0]] = 1
h[b[1]] = 2
p h.size
p h[b[0]]

# the entries survive collection: they are static, so the mark must skip them
kept = []
2000.times do |i|
  kept.push(b[i % 4]) if (i % 97).zero?
  raise "bad" unless b[i % 4].bytesize == 1
end
p kept.length
p kept.map { |c| c.bytes[0] }.uniq.sort
p b.bytes

# and the ASCII table is untouched
s = String.new("abc")
p s[1]
p s[1].encoding.to_s
p s[1] == "b"
p s[1].frozen?
