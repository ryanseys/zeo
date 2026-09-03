Point = Data.define(:x, :y)
p Point.new(*[1, 2])
p (Point.new(*[1]) rescue $!.class)
p (Point.new(*[1,2,3]) rescue $!.class)
S = Struct.new(:a, :b)
p S.new(*[1, 2]).to_a
p S.new(*[1]).to_a
__END__
#<data Point x=1, y=2>
ArgumentError
ArgumentError
[1, 2]
[1, nil]
