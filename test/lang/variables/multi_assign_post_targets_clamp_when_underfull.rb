# The same underfull clamping in a multi-assignment: `w, *x, y, z = [1, 2]`
# is `y=2, z=nil`, not `z=2`.

w, *x, y, z = [1, 2]
p [w, x, y, z]
a, *b, c = [1]
p [a, b, c]
q, r, *s, t, u = [1, 2, 3]
p [q, r, s, t, u]
a2, *b2, c2 = [1, 2, 3, 4]
p [a2, b2, c2]
__END__
[1, [], 2, nil]
[1, [], nil]
[1, 2, [], 3, nil]
[1, [2, 3], 4]
