# `send`, `respond_to?` and `method` all honour a refinement in ruby, and all
# three are ordinary Kernel methods any class may shadow. zeo folded the two
# questions at compile time, which works only where the receiver's class is
# known -- inside a method body a parameter's class is not, so a refined name
# was invisible to all three, and a class carrying its own `respond_to?` was
# answered over.
module R
  refine Array do
    def second = self[1]
  end
end
using R

# The receiver is a parameter: its class is a runtime fact.
def q(a) = a.respond_to?(:second)
def s(a) = a.send(:second)
def m(a) = a.method(:second).call
def mo(a) = a.method(:second).owner
def c(a) = a.second

p q([1, 2])
p s([1, 2])
p m([1, 2])
p mo([1, 2])
p c([1, 2])

# The same five where the class IS known, which already worked.
p [1, 2].respond_to?(:second)
p [1, 2].send(:second)
p [1, 2].method(:second).call
p [1, 2].method(:second).owner
p [1, 2].second

# A name no refinement defines still answers for the unrefined receiver.
p q([1, 2]) == q([1, 2])
p({ a: 1 }.respond_to?(:second))
def r(a) = a.respond_to?(:size)
p r([1, 2]), r("ab")

# A class that defines its OWN reflection entry wins the lookup, whether or
# not the compiler knows the receiver's class.
class Shadow
  def respond_to?(_n, _all = false) = :shadowed
  def method(_n) = :shadow_method
  def send(_n) = :shadow_send
end
def sh(o) = o.respond_to?(:anything)
def shm(o) = o.method(:anything)
def shs(o) = o.send(:anything)
p sh(Shadow.new), shm(Shadow.new), shs(Shadow.new)
p Shadow.new.respond_to?(:anything), Shadow.new.method(:anything), Shadow.new.send(:anything)

# `respond_to_missing?` keeps answering for a receiver of an unknown class.
class Ghost
  def respond_to_missing?(n, _all = false) = n == :spooky
end
def g(o) = o.respond_to?(:spooky)
p g(Ghost.new), Ghost.new.respond_to?(:spooky), g(Ghost.new) == Ghost.new.respond_to?(:boring)
__END__
true
2
2
#<refinement:Array@R>
2
true
2
2
#<refinement:Array@R>
2
true
false
true
true
:shadowed
:shadow_method
:shadow_send
:shadowed
:shadow_method
:shadow_send
true
true
false
