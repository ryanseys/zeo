# In Ruby `def self.foo` is an ordinary instance method on the SINGLETON
# class, and singleton classes form a parallel chain --
# #<Class:C>.super == #<Class:C.superclass> (make_metaclass,
# class.c:1186) -- so `super` needs no special case. Covers a three-level
# chain, explicit args, and zsuper forwarding of a defaulted parameter.

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
__END__
100
1000
1001
"hello MATZ!"
"t-foo"
"x-foo"
