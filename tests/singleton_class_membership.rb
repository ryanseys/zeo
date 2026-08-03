# A value IS an instance of its own singleton class. CRuby gets this free --
# `CLASS_OF(Foo)` literally IS `#<Class:Foo>`, and `#<Class:Sub>` inherits
# `#<Class:Foo>` in a parallel chain. zeo mints singleton classes on demand, so
# every reader of the relation has to answer it from the id's OWNER instead.
#
# `#class` still reports the real class: CRuby skips singletons there, which is
# why `instance_of?` says false where `is_a?` says true.

class Foo
  def self.a = 1
end
class Sub < Foo; end
class Other; end

sc = Foo.singleton_class

p Foo.is_a?(sc)
p Foo.kind_of?(sc)
p Sub.is_a?(sc)
p Other.is_a?(sc)
p Foo.new.is_a?(sc)
p Sub.is_a?(Sub.singleton_class)
p Foo.is_a?(Sub.singleton_class)

# `#class` and `instance_of?` deliberately look past the singleton.
p Foo.class
p Foo.instance_of?(sc)
p Foo.instance_of?(Class)

# `Module#===` is the same question, so `case` reaches it too.
p sc === Foo
p sc === Other
case Sub
when sc then puts "Sub matched #<Class:Foo>"
else puts "no match"
end

# An ordinary object's singleton class has exactly one instance -- identity,
# not equality.
class Bag
  def initialize(n) = @n = n
  def ==(other) = other.is_a?(Bag)
end
a = Bag.new(1)
b = Bag.new(2)
p a == b
p a.is_a?(a.singleton_class)
p b.is_a?(a.singleton_class)
p a.singleton_class.equal?(b.singleton_class)

# ...and the module chain above a singleton class still answers, unchanged.
p Foo.is_a?(Class)
p Foo.is_a?(Module)
p Foo.is_a?(Object)
