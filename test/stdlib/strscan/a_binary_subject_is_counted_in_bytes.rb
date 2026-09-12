# A StringScanner over binary bytes counts BYTES everywhere: its positions,
# its slices, and the encoding of everything it hands back.
require "strscan"

s = StringScanner.new("\xB5ab\xC3".b)
p s.string.encoding.to_s
p s.get_byte.bytes, s.pos
p s.peek(2).bytes, s.peek(2).encoding.to_s
p s.scan(/a/).bytes, s.pos
p s.scan_byte, s.peek_byte
p s.rest.bytes, s.rest_size

s.pos = 0
p s.pos, s.charpos
p s.scan_until(/b/).bytes, s.pos, s.matched_size
p s.pre_match.bytes, s.post_match.bytes
p s.bol?
__END__
"ASCII-8BIT"
[181]
1
[97, 98]
"ASCII-8BIT"
[97]
2
98
195
[195]
1
0
0
[181, 97, 98]
3
1
[181, 97]
[195]
false
