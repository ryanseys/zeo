class T1
  alias_method :eql?, :==
end
class T1
  def ==(other) = true
end
a, b = T1.new, T1.new
p [a == b, a.eql?(b), a.eql?(a)]
p T1.instance_methods(false).sort
p T1.instance_method(:eql?).owner.to_s
p a.method(:eql?).owner.to_s
p a.send(:eql?, b)

# BasicObject#== is identity, whatever the receiver's own `==` says.
class T2
  def ==(other) = true
end
c, d = T2.new, T2.new
p [c == d, BasicObject.instance_method(:==).bind(c).call(d)]

# The alias still follows a source the SAME class defined EARLIER.
class T3
  def label = "first"
  alias_method :tag, :label
  def label = "second"
end
p [T3.new.label, T3.new.tag]
