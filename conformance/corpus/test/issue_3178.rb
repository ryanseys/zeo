h = { a: ->(x) { "got #{x}" } }
p h[:a].call(*["z"])

class D
  def initialize; @h = Hash.new { |x, k| x[k] = [] }; end
  def on(e, &b); @h[e] << b; end
  def emit(e, *a); @h[e].map { |p| p.call(*a) }; end
end
d = D.new
d.on(:x) { |u| "hi #{u}" }
p d.emit(:x, "al")

procs = [->(a, b) { a + b }, ->(a, b) { a * b }]
p procs.map { |pr| pr.call(*[3, 4]) }
