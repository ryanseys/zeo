# `Integer#chr` allocated a 1-byte string per call -- 64% of a chr loop was
# malloc, free and the GC sweep walking what they left. The static table that
# now serves 1-byte slices serves this too, so nothing is allocated.
#
# It stays on the PLAIN table, not the BINARY one. spinel does not let a
# program name an encoding, so nothing but pack and String#b asks for BYTES,
# and a chr result is not one of those. Tagging it BINARY would have matched
# CRuby on two more surfaces (255.chr.encoding, 1.chr.inspect) and diverged on
# two others; leaving it plain changes nothing observable at all.
#
# The table entry has a real header, so byte 0 is a 1-byte string here where
# strlen made it the empty one.

p 0.chr.length
p 0.chr.bytesize
p 0.chr.ord
p 0.chr.bytes
p 65.chr
p 65.chr.length
p 65.chr.bytes
p 65.chr.ord
p 255.chr.bytes
p 255.chr.bytesize
p 255.chr.ord
p 127.chr.ord

# building with chr
p 65.chr + 66.chr
p (65.chr + 66.chr).length
p [72, 105].map { |c| c.chr }.join
s = String.new("x")
s << 65.chr
p s
p s.length
p 65.chr == "A"
p "abc".include?(97.chr)
p "abc".index(98.chr)
p 65.chr.to_sym == :A

# a chr result is mutable, and mutating a copy must not poison the entry
p 65.chr.frozen?
t = 65.chr.dup
t << "z"
p t
p 65.chr
w = 65.chr.dup
w.setbyte(0, 66)
p w
p 65.chr

# embedded NUL through concat, the shape #593 is about
b = 0.chr + 0xc8.chr
p b.length
p b.bytesize
p b[0].ord
p b[1].ord

# as Hash keys, and against a literal
h = {}
h[65.chr] = 1
h["A"] = 2
p h.size
p [65.chr, "A"].uniq.length

# the entries are static: a collection must not reach them
kept = []
2000.times do |i|
  kept.push((i % 256).chr) if (i % 97).zero?
  raise "bad" unless (i % 256).chr.bytesize == 1
end
p kept.length
p kept.map { |c| c.bytes[0] }.uniq.length
p 65.chr.bytes
