# `s + s` hands the SAME Arc<Mutex<..>> in as both receiver and argument.
# parking_lot's Mutex is not reentrant, so any implementation taking both
# guards in one expression deadlocks -- the process hangs with no output
# and no error, which is strictly worse than a wrong answer. Every
# self-argument shape is covered here because the bug is silent: it
# surfaces as a timeout, never as a failed assertion.

s = "ab"
p s + s
p s * 2
p s.concat(s)
p Encoding.compatible?(s, s)
a = [1, 2]
p(a <=> a)
p a == a
p a + a
p a - a
p a & a
p a | a
h = {x: 1}
p h == h
p h.merge(h)
r = Random.new(5)
p r == r
__END__
"abab"
"abab"
"abab"
#<Encoding:UTF-8>
0
true
[1, 2, 1, 2]
[]
[1, 2]
[1, 2]
true
{x: 1}
true
