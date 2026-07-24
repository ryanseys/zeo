pr = proc { |a, b = 1, *c, d:, e: 2, **f, &g| }
p pr.parameters
la = ->(a, b = 1) {}
p la.parameters
def m117(x); x * 2; end
mo = method(:m117)
pc = mo.to_proc
p pc.call(5)
add = proc { |a, b| a + b }
p add.yield(1, 2)
# bound method to_proc
class C2116
  def initialize(n); @n = n; end
  def scale(x); x * @n; end
  def label(s); "#{s}:#{@n}"; end
end
obj = C2116.new(3)
sp = obj.method(:scale).to_proc
p sp.call(7)
lp = obj.method(:label).to_proc
p lp.call("id")
# string-returning top-level
def greet(name); "hello #{name}"; end
gp = method(:greet).to_proc
p gp.call("matz")
p gp.arity
p gp.lambda?
# parameters edge: anonymous + numbered
p proc { |*| }.parameters
p proc { |**| }.parameters
p ->(a, *, &blk) {}.parameters
np = proc { _2 }
p np.parameters
# post params
p ->(a, *m, z) {}.parameters
# builtin operator bound method: __bam wrapper with self-carried param
bm = 5.method(:+)
p bm.call(3)
bp = bm.to_proc
p bp.call(10)
p bp.arity
sm = "ab".method(:*)
p sm.call(2)
