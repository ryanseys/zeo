# A NUL is an ordinary byte in a Ruby String, but repeat, unary plus, the
# justify family and the character walk all sized themselves with strlen and
# so dropped the NUL and everything after it. #3473.
s = "a\0b"
p((s * 2).bytes)
p((+s).bytes)
p(s.center(7, "-").bytes)
p(s.ljust(6, "-").bytes)
p(s.rjust(6, "-").bytes)
p(s.center(7).bytes)
p(s.ljust(6).bytes)
p(s.rjust(6).bytes)
p(s.chars.map(&:bytes))
p(s.each_char.to_a.map(&:bytes))
n = 0
s.each_char { |ch| n += 1 }
p n
u = 0.chr * 3
p(u.bytes)
p(u.length)
p(("" + 0.chr).ljust(3, "x").bytes)
# the copy unary plus hands back is mutable, and keeps the NUL through an append
c = +s
c << "z"
p(c.bytes)
