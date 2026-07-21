# A BasicObject subclass is a blank slate (#2703): it answers only
# BasicObject's own methods (==, equal?, instance_eval, ...) and what the user
# defined; the Object/Kernel surface raises NoMethodError.
class BO < BasicObject
  def greet; "hi"; end
end
a = BO.new
r = (a.class rescue $!.class)
p r
p a.greet
r2 = (a.inspect rescue "no-inspect")
p r2
r3 = (a.respond_to?(:greet) rescue "no-respond_to")
p r3
p(a == a)
b = BO.new
p(a == b)
p(a.equal?(a))
class Normal; end
p Normal.new.class
