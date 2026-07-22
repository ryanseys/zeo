# Blockless Kernel#loop returns an infinite Enumerator (yields nil).
e = loop
p e.class
p loop.first(3)
p loop.take(2)
e2 = loop
p e2.next
p e2.next
