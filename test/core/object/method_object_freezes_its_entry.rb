# A `Method`/`UnboundMethod` COPIES the method entry when it is taken, so a
# later redefinition cannot change what it calls. That single fact is what makes
# the wrap-and-redefine idiom terminate -- the canonical thing a `method_added`
# hook is written to do. Re-resolving by name instead finds the wrapper and
# recurses until the stack runs out.

puts "== UnboundMethod#bind"
class Z
  def m = "orig"
end
orig = Z.instance_method(:m)
Z.send(:define_method, :m) { "wrapped(" + orig.bind(self).call + ")" }
p Z.new.m

puts "== UnboundMethod#bind_call"
class Y
  def m = "orig"
end
o2 = Y.instance_method(:m)
Y.class_eval { define_method(:m) { "w(" + o2.bind_call(self) + ")" } }
p Y.new.m

puts "== Object#method"
class W
  def m = "orig"
end
w = W.new
bound = w.method(:m)
W.send(:define_method, :m) { "new" }
p [bound.call, w.m]

puts "== unbind and rebind still carries the frozen entry"
class V
  def m = "orig"
end
v1, v2 = V.new, V.new
taken = v1.method(:m)
V.send(:define_method, :m) { "new" }
p [taken.unbind.bind(v2).call, v2.m]

puts "== the whole wrap-and-redefine hook, which is the point"
class Traced
  def self.method_added(name)
    return if @wrapping
    @wrapping = true
    inner = instance_method(name)
    define_method(name) { |*a| "<#{inner.bind(self).call(*a)}>" }
    @wrapping = false
  end

  def first = "1st"
  def later(n) = "2nd:#{n}"
end
p [Traced.new.first, Traced.new.later(7)]

puts "== super_method still resolves down the chain"
class Base
  def greet = "base"
end
class Sub < Base
  def greet = "sub+" + super
end
mm = Sub.new.method(:greet)
p [mm.call, mm.super_method.call, mm.owner, mm.super_method.owner]

puts "== an alias keeps its own entry"
class A
  def m = "first"
  alias_method :copy, :m
  def m = "second"
end
p [A.new.m, A.new.copy]
__END__
== UnboundMethod#bind
"wrapped(orig)"
== UnboundMethod#bind_call
"w(orig)"
== Object#method
["orig", "new"]
== unbind and rebind still carries the frozen entry
["orig", "new"]
== the whole wrap-and-redefine hook, which is the point
["<1st>", "<2nd:7>"]
== super_method still resolves down the chain
["sub+base", "base", Sub, Base]
== an alias keeps its own entry
["second", "first"]
