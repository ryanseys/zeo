# Array#[start, len] = replacement across element kinds: the equal-length
# overwrite (the fast path) and the growing / shrinking / clamped forms.
a = [1, 2, 3, 4, 5]
a[1, 3] = [9, 9, 9]
p a
a[0, 2] = [7]
p a
a[2, 0] = [5, 5]
p a
b = ["x", "y", "z"]
b[1, 2] = ["p", "q"]
p b
c = [1.5, 2.5, 3.5]
c[0, 2] = [9.5, 8.5]
p c
d = [1, "a", :b]
d[1, 1] = ["z"]
p d
e = [1, 2, 3]
e[1, 5] = [8]
p e
__END__
[1, 9, 9, 9, 5]
[7, 9, 9, 5]
[7, 9, 5, 5, 9, 5]
["x", "p", "q"]
[9.5, 8.5, 3.5]
[1, "z", :b]
[1, 8]
