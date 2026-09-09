# Method#owner and UnboundMethod#owner answer the singleton class for a
# `def self.` method, on a user class and a builtin, and through
# `singleton_class.instance_method`.

class Foo
  def self.a = 1
  def b = 2
end

p Foo.method(:a).owner
p Time.method(:now).owner
p Foo.singleton_class.instance_method(:a).owner

# An INSTANCE method is unaffected, which is what makes this a singleton-branch
# fix rather than a blanket one.
p Foo.instance_method(:b).owner
p Foo.new.method(:b).owner
p Foo.method(:name).owner

# The answer is a real class object, and the same one `singleton_class` gives.
p Foo.method(:a).owner.equal?(Foo.singleton_class)
p Foo.method(:a).owner.class
p Foo.singleton_class.instance_method(:a).name
p Foo.method(:a).unbind.owner

# A module's class method, and an inherited one.
module Util
  def self.helper = :helped
end
class Sub < Foo; end
p Util.method(:helper).owner
p Sub.method(:a).owner
__END__
#<Class:Foo>
#<Class:Time>
#<Class:Foo>
Foo
Foo
Module
true
Class
:a
#<Class:Foo>
#<Class:Util>
#<Class:Foo>
