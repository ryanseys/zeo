p((1..).each.next)
e = (1..).each
p e.next
p e.next
p e.peek
p e.next
e.rewind
p e.next
f = (10..).each
p f.next
p f.next
g = (1..3).each
p g.next
p g.next
p g.next
r = (g.next rescue $!.class)
p r
