P = Data.define(:x, :y)
a = P.new(1, 2)
r = (a.with(z: 9) rescue $!.class); p r
