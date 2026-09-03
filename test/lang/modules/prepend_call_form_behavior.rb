# Comprehensive behavior of the `C.prepend(M)` call-form desugar: reflection
# (is_a?/kind_of?/ancestors/include?), diamond expansion (a prepended module
# that itself includes another), multiple separate prepend calls, interaction
# with a class-body `prepend` on the same class, and `super` from a prepended
# module reaching an INCLUDED module's method -- all resolved through the
# ordinary compile-time-flattened MRO, matching ruby.

# reflection after a prepend call
module Track
  def step
    "T:" + super
  end
end
class Flow
  def step
    "flow"
  end
end
Flow.prepend(Track)
f = Flow.new
puts f.is_a?(Track)
puts f.kind_of?(Flow)
puts Flow.ancestors.inspect
puts Flow.include?(Track)
puts f.step

# a prepended module that itself includes another (diamond expansion)
module Base
  def label
    "base"
  end
end
module Deco
  include Base
  def label
    "deco(" + super + ")"
  end
end
class Card
  def label
    "card"
  end
end
Card.prepend(Deco)
puts Card.ancestors.inspect
puts Card.new.label

# multiple SEPARATE prepend calls: the later one is closer
module P
  def tag
    "P" + super
  end
end
module Q
  def tag
    "Q" + super
  end
end
class Item
  def tag
    "item"
  end
end
Item.prepend(P)
Item.prepend(Q)
puts Item.ancestors.first(4).inspect
puts Item.new.tag

# a prepend call interacting with a class-body prepend on the same class
module Early
  def val
    "E" + super
  end
end
module Late
  def val
    "L" + super
  end
end
class Mix
  prepend Early
  def val
    "mix"
  end
end
Mix.prepend(Late)
puts Mix.ancestors.first(4).inspect
puts Mix.new.val

# super from a prepended module reaching an INCLUDED module's method
module Common
  def go
    "common"
  end
end
class Runner
  include Common
end
module Hook
  def go
    "hook(" + super + ")"
  end
end
Runner.prepend(Hook)
puts Runner.new.go
puts Runner.ancestors.inspect
__END__
true
true
[Track, Flow, Object, Kernel, BasicObject]
true
T:flow
[Deco, Base, Card, Object, Kernel, BasicObject]
deco(base)
[Q, P, Item, Object]
QPitem
[Late, Early, Mix, Object]
LEmix
hook(common)
[Hook, Runner, Common, Object, Kernel, BasicObject]
