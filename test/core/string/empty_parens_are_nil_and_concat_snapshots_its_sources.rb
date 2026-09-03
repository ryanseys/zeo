# `()` is nil (falsy as a condition, a nil value in expression position);
# `Array#concat` copies all sources before appending, so a self-aliasing
# `a.concat(a, a)` terminates at 6 elements rather than feeding itself.

p(())
p((() && true))
n = 0
while () ; n += 1 ; end
p n
a = [1, 2]
a.concat(a, a)
p a
__END__
nil
nil
0
[1, 2, 1, 2, 1, 2]
