def greet3(g, n) = "#{g}, #{n}!"
cg = method(:greet3).to_proc.curry
p cg["Hi"]["there"]
p cg["Hi", "world"]

Point = Struct.new(:x, :y)
def combine(a, b, c) = "#{a.x}-#{b}-#{c}"
f = method(:combine).to_proc.curry
p f[Point.new(1, 2)]["mid"][3]

g = ->(a, b) { "#{a}/#{b}" }.curry
p g["x"]["y"]

def add3(a, b, c) = a + b + c
h = method(:add3).to_proc.curry
p h[1][2][3]
