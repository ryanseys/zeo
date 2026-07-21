# Module#instance_method yields an UnboundMethod (#2724): #class says so, and
# calling it without a #bind is CRuby's NoMethodError. Binding restores the
# ordinary Method behavior.
class Animal; def breathe; "air"; end; end
m = Animal.instance_method(:breathe)
p m.class
r = (m.call rescue $!.class)
p r
p m.bind(Animal.new).call
p m.bind(Animal.new).class
