# (1..).each answers an Enumerator whose next and peek keep advancing.
# (spinel issue #3229)
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
__END__
1
1
2
3
3
1
10
11
1
2
3
StopIteration
