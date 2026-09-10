# One member, both, a mix of keyword and splat, and an empty splat.
Point = Data.define(:x, :y)
a = Point.new(1, 2)
p a.with(**{x: 10})
p a.with(**{x: 10, y: 20})
p a.with(y: 5, **{x: 9})
p a.with(**{})
__END__
#<data Point x=10, y=2>
#<data Point x=10, y=20>
#<data Point x=9, y=5>
#<data Point x=1, y=2>
