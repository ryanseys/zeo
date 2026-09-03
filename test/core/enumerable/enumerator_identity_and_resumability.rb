# Enumerator identity/copy semantics: blockless `each` returns SELF,
# pre-iteration dup is a fresh enumerator over the same source, breaking
# out of an external loop leaves the enumerator resumable.

e = [1, 2].each
p e.each.equal?(e)
d = e.dup
p d.class
p e.next
p d.next
g = [1, 2, 3].each
loop do
  v = g.next
  break if v == 2
end
p g.next
__END__
true
Enumerator
1
1
3
