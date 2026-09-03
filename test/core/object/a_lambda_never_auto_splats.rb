# A lambda is strict: it never auto-splats, whatever its param shape.

p(->(a) { a }.call([1, 2]))
p(->(a, b) { [a, b] }.call(1, 2))
__END__
[1, 2]
[1, 2]
