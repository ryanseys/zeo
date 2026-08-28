# The value of a setter call, `obj.x = v`: the argument as written, in value
# position, at a method's tail, and through a receiver whose class is only
# known at run time, for hand-written writers and attr_writer alike.
# CRuby generated the expectations.
class Node
  attr_reader :label, :weight
  attr_accessor :tag
  def initialize; @label = ""; @weight = 0; @tag = nil; end
  def label=(v); @label = v.to_s; :ignored; end
  def weight=(v); @weight = v; 99; end
  def set_weight(v); self.weight = v; end        # tail position, hand-written
  def set_tag(v); self.tag = v; end              # tail position, attr_accessor
end

n = Node.new
p(n.label = 42)          # the Integer as written, not the String stored
p n.label
p(n.weight = 7)
x = (n.weight = 8); p x
y = n.weight = 20; p y
p n.set_weight(10)
p n.set_tag(11)
p(n.weight = nil)
p(n.weight = "s")
p(n.tag = [1, 2])
n.weight = 13; p n.weight   # statement position stores as before

# a receiver whose class is only known at run time
r = [Node.new, "x"][0]
p(r.weight = 5)
p(r.tag = 6)
def poly_tail(o); o.weight = 7; end
p poly_tail(r)
def poly_tail_attr(o); o.tag = 8; end
p poly_tail_attr(r)
p(r.weight = nil)
p(r.label = 3.5)
p r.label
w = (r.weight = "poly-str"); p w
begin
  q = [Node.new, "x"][1]
  q.weight = 1
rescue NoMethodError => e
  puts "NoMethodError: #{e.message[0, 25]}"
end
