# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# A D holds `@h`, whose default proc closed over the D.
#@ gccheck: cycle leak: 5 objects (Proc x2, Array x1, D x1, Hash x1)
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
__END__
"got z"
["hi al"]
[7, 12]
