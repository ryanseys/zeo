# Regexp#hash covers the flags: /ab/ and /ab/i are not eql?, so they must not
# collide as Hash keys (#3816).
p(/ab/.hash == /ab/i.hash)
p(/ab/.hash == /ab/.hash)
p(/ab/.eql?(/ab/i))
p(/ab/.eql?(/ab/))
h = { /ab/ => 1 }
h[/ab/i] = 2
p h.size
p h[/ab/]
p h[/ab/i]
