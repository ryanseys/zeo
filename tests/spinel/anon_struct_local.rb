# Struct.new(:a, :b) / Data.define(:x, :y) assigned DIRECTLY to a local
# (never through a constant): register_structs synthesizes an anonymous
# struct class keyed to the write node, class_var_static_ci resolves the
# local's reads to it (so .new/.members and the member accessors dispatch
# like the constant form), the value itself is a first-class class object,
# and #inspect omits the synthetic name like CRuby (#<struct a=1, b=2>).
k144 = Struct.new(:a, :b)
obj = k144.new(1, 2)
p(obj.a)
p(obj.b)
obj.b = 20
p(obj.b)
p(k144.members)
p obj
p obj.to_a

d = Data.define(:x, :y)
dv = d.new(x: 5, y: 6)
p dv.x
p dv

pair = Struct.new(:l, :r)
p1 = pair.new("L", "R")
p p1.sum { |v| v.length }
p(p1.map { |v| v * 2 })

begin
  k144.new(1, 2, 3)
rescue ArgumentError => e
  puts "AE: #{e.message}"
end
