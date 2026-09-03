# Three class-model behaviors that follow from Ruby's real object model.

# 1. `super` in a CLASS method. In Ruby `def self.foo` is an ordinary instance
#    method on the SINGLETON class, and singleton classes form a parallel
#    chain (#<Class:C>.super == #<Class:C.superclass>), so `super` needs no
#    special case -- including through a three-level chain and with zsuper
#    argument forwarding.
class Foo
  def self.base; 100; end
  def self.greet(name); "hello #{name}"; end
  def self.tag(pfx = "t"); pfx + "-foo"; end
end
class Bar < Foo
  def self.base; super * 10; end
  def self.greet(name); super(name.upcase) + "!"; end
  def self.tag(pfx = "t"); super; end
end
class Baz < Bar
  def self.base; super + 1; end
end
p Foo.base
p Bar.base
p Baz.base
p Bar.greet("matz")
p Bar.tag
p Bar.tag("x")

# 2. A BasicObject subclass is a BLANK SLATE. Kernel is mixed into Object, so
#    it sits BELOW BasicObject in the chain and a BasicObject subclass never
#    sees it -- the whole Object/Kernel surface is simply absent. Note `send`
#    and `public_send` live on Kernel; only `__send__` is BasicObject's.
class BO < BasicObject
  def initialize; @x = 1; end
  def greet; "hi"; end
  def own; @x; end
end
a = BO.new
r1 = (a.class rescue $!.class); p r1
p a.greet
r2 = (a.inspect rescue "no-inspect"); p r2
r3 = (a.respond_to?(:greet) rescue "no-respond_to"); p r3
r4 = (a.send(:greet) rescue $!.class); p r4
p a.__send__(:greet)
p(a == a)
p(a == BO.new)
p a.equal?(a)
r5 = (a.dup.own rescue $!.class); p r5
p a.own

# an ordinary class still has the full surface
class Normal; end
p Normal.new.class
p Normal.new.respond_to?(:inspect)
__END__
100
1000
1001
"hello MATZ!"
"t-foo"
"x-foo"
NoMethodError
"hi"
"no-inspect"
"no-respond_to"
NoMethodError
"hi"
true
false
true
NoMethodError
1
Normal
true
