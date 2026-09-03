Point = Data.define(:x, :y)
a = Point.new(1, 2)
b = Point.new(x: 1, y: 2)
p [a.x, a.y]
p(a == b)
p a.frozen?
# The `rescue` modifier needs its own parens inside a call's arguments
# (a bare `p(x rescue y)` is a SyntaxError in ruby 4.0.6 too).
p((Point.new(1) rescue $!.class))
p((Point.new(x: 1, y: 2, z: 3) rescue $!.class))
p((a.with(z: 9) rescue $!.class))
p a.with(y: 5).to_h
__END__
[1, 2]
true
true
ArgumentError
ArgumentError
ArgumentError
{x: 1, y: 5}
