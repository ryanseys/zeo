class Box
  def initialize; @v = 1; @s = "hi"; end
  def drop; remove_instance_variable(:@v); end
end
b = Box.new
p b.drop
p(begin; b.remove_instance_variable(:@nope); rescue NameError => e; e.message; end)
p "x".singleton_class.class
p Object.new.singleton_class.superclass
o = Object.new
def o.greet; "hey"; end
p o.singleton_method(:greet).call
sc = Object.new
sc.singleton_class.define_method(:doubled) { 21 * 2 }
p sc.doubled
module Greet; def hi(n); "hi #{n}"; end; end
u = Object.new
u.extend(Greet)
p u.hi("ada")
n = Object.new
n.extend(Comparable)
p n.respond_to?(:clamp)
