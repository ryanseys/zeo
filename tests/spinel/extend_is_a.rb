# `extend` changes what a receiver IS, not only what it answers: the module
# joins that ONE object's singleton chain, so `is_a?`, `===`, `case`/`when`,
# a `ClassCheck` pattern and `singleton_class.ancestors` all see it while the
# receiver's class -- and every other instance of it -- stay untouched.

module Greet
  def greet = "hey #{name}"
end

module Tagged; end

module Nested
  include Tagged
end

class Person
  def initialize(name) = @name = name
  def name = @name
end

a = Person.new("a")
b = Person.new("b")
a.extend(Greet)

puts a.greet
p a.is_a?(Greet)
p b.is_a?(Greet)
p a.kind_of?(Greet)
p Greet === a
p Greet === b
p a.instance_of?(Greet)
p a.class
p a.class.ancestors.include?(Greet)
p a.singleton_class.ancestors.include?(Greet)
p Person.new("c").is_a?(Greet)

# Through a call, so the receiver is not statically known at the call site.
def greetable?(x) = x.is_a?(Greet)
p greetable?(a)
p greetable?(b)

# `case`/`when` and a pattern-match `in` reach the same answer.
[a, b].each do |x|
  case x
  when Greet then puts "when: yes"
  else puts "when: no"
  end
end
[a, b].each do |x|
  case x
  in Greet then puts "in: yes"
  else puts "in: no"
  end
end
p [a, b].grep(Greet).size

# An extended module carries its own includes.
n = Person.new("n")
n.extend(Nested)
p n.is_a?(Nested)
p n.is_a?(Tagged)

# Non-Object receivers: a bare collection, a string, and nil all take one.
p [1, 2].extend(Tagged).is_a?(Tagged)
p "s".extend(Tagged).is_a?(Tagged)
p nil.extend(Tagged).is_a?(Tagged)
begin
  5.extend(Tagged)
rescue TypeError => e
  puts "TypeError: #{e.message}"
end

# A CLASS receiver, both spellings. `extend` in the body and a later
# `.extend` call put the module on the class's singleton chain -- an
# INSTANCE of the class is unaffected either way.
module Registry
  def registered = "registered #{self}"
end

class Widget
  extend Registry
end

class Gadget; end
Gadget.extend(Registry)

puts Widget.registered
puts Gadget.registered
p Widget.is_a?(Registry)
p Gadget.is_a?(Registry)
p Widget.singleton_class.ancestors.include?(Registry)
p Gadget.singleton_class.ancestors.include?(Registry)
p Widget.new.is_a?(Registry)
p Gadget.new.is_a?(Registry)

# `extend self` -- the module-function idiom, whose whole point is that the
# module becomes an instance of itself.
module Util
  extend self
  def double(n) = n * 2
end
p Util.double(21)
p Util.is_a?(Util)

# Two extends layer, newest closest to the singleton, and a repeat is a no-op.
module First
  def which = "first"
end
module Second
  def which = "second"
end
layered = Person.new("l")
layered.extend(First)
layered.extend(Second)
layered.extend(First)
puts layered.which
p layered.is_a?(First)
p layered.is_a?(Second)
p layered.singleton_class.ancestors.take(3).drop(1)

# A singleton class asked for BEFORE the extend reports the same chain as one
# asked for after -- it is one class, not two.
late = Person.new("late")
sc = late.singleton_class
late.extend(Greet)
p sc.equal?(late.singleton_class)
p sc.ancestors.include?(Greet)

# `dup` drops the extension, as it drops every singleton.
p a.dup.is_a?(Greet)
