# Embedded NUL bytes survive frozen-string operations (dedup / .freeze): the
# 0xf1 frozen marker carries the real byte length, so nothing truncates at NUL.
s = "a\u0000b"
p s.bytesize
p(s == "a\u0000c")
p(s == "a\u0000b")
d = s.dedup
p d.bytesize
p(d == s)
p("a\u0000b".dedup.equal?("a\u0000b".dedup))
f = "x\u0000y".freeze
p f.bytesize
h = { s => 1 }
h["a\u0000c"] = 2
p h.size
