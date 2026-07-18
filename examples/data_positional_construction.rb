# Data.define values construct positionally or by keyword, are frozen, and
# validate unknown/missing members.
Point = Data.define(:x, :y)

pos = Point.new(1, 2)
kw  = Point.new(x: 1, y: 2)
p [pos.x, pos.y]
p (pos == kw)
p pos.frozen?
p pos.to_h
p pos.with(y: 9).to_h

p (Point.new(1) rescue $!.class)
p (Point.new(x: 1) rescue $!.class)
p (Point.new(x: 1, y: 2, z: 3) rescue $!.class)
p (pos.with(z: 9) rescue $!.class)
