# `<=>` between self-referential arrays uses CRuby's PAIRED recursion
# guard, the same one `==` already had. Without it the walk recursed until
# the stack ran out and the process aborted.
#
# A revisited pair does NOT answer 0. CRuby's `recursive_cmp` hands back
# `Qundef` and `rb_ary_cmp` falls through to the LENGTH comparison, so the
# two agree for equal-length arrays and differ otherwise -- which is what
# the third row below is for.

a = [1]
a << a
b = [1]
b << b
p a == b
p(a <=> b)

# Different lengths: the length comparison is what answers, not 0.
c = [1]
c << c
d = [1]
d << d << 3
p(c <=> d)
p(d <=> c)

# The pair is ORDERED, so `a <=> b` and `b <=> a` are different
# comparisons and both terminate.
p(b <=> a)

# A shared element is not a cycle, and still compares element-wise.
shared = [1, 2]
p([shared, 3] <=> [shared, 4])
p([shared] <=> [shared])

# Ordinary comparisons are untouched.
p([1, 2] <=> [1, 3])
p([1, 2] <=> [1, 2, 3])
p([[1], [2]] <=> [[1], [2]])
p(([1] <=> ["a"]).inspect)
