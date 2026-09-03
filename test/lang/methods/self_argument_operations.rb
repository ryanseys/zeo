# Operations whose argument IS their receiver. These pass one shared mutable
# object in twice, so an implementation that takes both locks at once hangs
# the process -- a silent failure that shows up only as a timeout.
s = "ab"
p s + s
p s * 2
a = [1, 2]
p(a <=> a)
p a == a
p a + a
p a - a
p a & a
p a | a
p a.concat(a)
h = {x: 1}
p h == h
p h.merge(h)
r = Random.new(5)
p r == r
p "ab".eql?("ab")
p Encoding.compatible?(s, s)
p s.concat(s)
__END__
"abab"
"abab"
0
true
[1, 2, 1, 2]
[]
[1, 2]
[1, 2]
[1, 2, 1, 2]
true
{x: 1}
true
true
#<Encoding:UTF-8>
"abab"
