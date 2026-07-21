Point = Data.define(:x, :y)
h = { x: 1, y: 2 }
p(Point.new(**h))
p(Point.new(**{ x: 3, y: 4 }))
