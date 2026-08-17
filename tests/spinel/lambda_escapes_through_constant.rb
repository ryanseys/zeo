# A lambda held in a constant (or an ivar) and passed to a method that calls
# it escapes: its parameters ride the boxed ABI. The escape scan followed only
# a local, so the literal kept the no-evidence int default and an Array
# argument was read as an integer -- every call answered 0 (#3968).
A = ->(x) { x[0] }
B = ->(_x) { 0 }
def run(h) = h.call([9])
p run(A)
p run(B)

C = ->(a, b) { a[0] + b[0] }
D = ->(a, b) { a[1] + b[1] }
def run2(h) = h.call([1, 2], [3, 4])
p run2(C)
p run2(D)

def manhattan(a, b) = ((a[0] - b[0]).abs + (a[1] - b[1]).abs).to_f
ASTAR = ->(a, b) { manhattan(a, b) }
DIJKSTRA = ->(_a, _b) { 0.0 }
def run3(h) = h.call([0, 0], [3, 4])
p run3(ASTAR)
p run3(DIJKSTRA)

# an ivar-held lambda passed on
class Holder
  def initialize
    @f = ->(x) { x[1] }
  end
  def use(g) = g.call([7, 8])
  def go = use(@f)
end
p Holder.new.go

# a directly-called constant lambda keeps its precise typing
E = ->(n) { n * 2 }
p E.call(21)
