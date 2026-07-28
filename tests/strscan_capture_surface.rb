require "strscan"

s = StringScanner.new("name=zeo; version=4")
p s.scan(/(\w+)=(\w+)/)
p s[0], s[1], s[2], s[-1], s[9]
p s.captures
p s.values_at(0, 2, 7)
p s.size
p s.matched_size
p s.pre_match, s.post_match

named = StringScanner.new("2026-07-28")
named.scan(/(?<y>\d{4})-(?<m>\d{2})/)
p named[:y], named["m"]
p named.named_captures
begin
  named[:nope]
rescue IndexError => e
  puts e.message
end

# A miss clears every register.
miss = StringScanner.new("abc")
miss.scan(/(a)/)
p miss.scan(/\d/)
p miss.matched?, miss.matched, miss.captures, miss.size, miss.matched_size
p miss[0]

# Positions: `pos` counts bytes, `charpos` counts characters.
multi = StringScanner.new("héllo wörld")
multi.scan(/h.l+o/)
p multi.pos, multi.charpos, multi.rest_size, multi.rest?
p multi.pointer
multi.pointer = 0
p multi.pos

p StringScanner.new("abc").inspect
p StringScanner.new("").inspect
spent = StringScanner.new("abc")
spent.terminate
p spent.inspect
long = StringScanner.new("a" * 30)
long.scan(/a{5}/)
p long.inspect

# The `*_full` primitives, and the two length-returning walkers.
full = StringScanner.new("hello")
p full.scan_full(/he/, false, true), full.pos
p full.scan_full(/he/, true, false), full.pos
p full.search_full(/ll/, true, true), full.pos

p StringScanner.new("abc;def").skip_until(/;/)
p StringScanner.new("abc").skip_until(/;/)

# Byte-level reads.
bytes = StringScanner.new("Zx")
p bytes.peek_byte, bytes.scan_byte, bytes.scan_byte, bytes.scan_byte
p StringScanner.new("+12ab").scan_integer
p StringScanner.new("-0x1f").scan_integer(base: 16)
p StringScanner.new("ab").scan_integer

# Appending feeds a scanner incrementally; replacing restarts it.
feed = StringScanner.new("ab")
feed.scan(/a/)
feed << "cd"
p feed.string, feed.pos
feed.string = "zz"
p feed.string, feed.pos
p feed.fixed_anchor?
p StringScanner.new("x", fixed_anchor: true).fixed_anchor?
