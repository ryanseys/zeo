# Class-level state: `self` inside a class method, and the instance
# variables of the CLASS OBJECT itself.
#
# The one rule everything here follows from: in a class method (or a class
# body), `self` is the class object. So `@x` there is that object's own ivar
# -- a different thing from both `@@x` and an instance's `@x` -- and an
# implicit-self call is a send to whichever class was actually called.

# --- `self` is the class object ---------------------------------------------
class Foo
  def self.who
    self
  end
end
p Foo.who
p Foo.who == Foo
p Foo.who.name
p Foo.who.new.class

# --- @x on the class object -------------------------------------------------
# A bare `@x = ...` in a class body seeds it; `def self.x` reads it.
class Registry
  @items = []
  @count = 0

  def self.add(x)
    @items << x
    @count += 1
    self          # so calls chain
  end

  def self.items = @items
  def self.count = @count
end

Registry.add("a").add("b").add("c")
p Registry.items
p Registry.count

# Reading one never written is nil -- no NameError (that's `@@x`'s rule, not
# this one).
class Quiet
  def self.probe = @never_written
end
p Quiet.probe

# --- NOT inherited (the difference from @@x) --------------------------------
# A subclass inherits the METHOD but gets its own, empty storage. A class
# variable, by contrast, is genuinely shared.
class Base
  @tag = "base"
  @@shared = "shared"
  def self.tag = @tag
  def self.tag=(v)
    @tag = v
  end
  def self.shared = @@shared
end
class Sub < Base; end

p [Base.tag, Sub.tag]              # Sub's slot is empty...
p [Base.shared, Sub.shared]        # ...but the cvar is one slot, shared
Sub.tag = "sub"
p [Base.tag, Sub.tag]              # writes stay independent

# --- a class @x and an instance @x are different slots ----------------------
# `def self.x` and `def x` are two namespaces in Ruby, and their `@x`s name
# two different objects' storage.
class Dual
  @x = "class-level"
  def initialize
    @x = "instance-level"
  end
  def self.x = @x
  def x = @x
end
p [Dual.x, Dual.new.x]

# --- `class << self` -- the idiomatic declaration ---------------------------
# `attr_accessor` there generates `def self.handler; @handler; end`, so it is
# class-level ivars all the way down.
module Plugin
  class << self
    attr_accessor :handler
    attr_reader :registered

    def register(h)
      @registered ||= []
      @registered << h
      self.handler = h
    end
  end
end

Plugin.register("first")
Plugin.register("second")
p Plugin.handler
p Plugin.registered

# --- module-level state -----------------------------------------------------
module Counter
  @n = 0
  def self.bump = @n += 1
  def self.n = @n
end
Counter.bump
Counter.bump
p Counter.n

# --- an implicit-self call resolves against the RECEIVER --------------------
# `new` and `name` below are sends to `self`, and `self` is whichever class
# was called -- not the one whose body the method was written in.
class Shape
  def self.build = new
  def self.label = "a #{name}"
end
class Circle < Shape; end

p Shape.build.class
p Circle.build.class               # Circle, not Shape
p Circle.label

module Greeter
  def greet = "hello from #{name}"
end
class English
  extend Greeter
end
p English.greet                    # names English, not Greeter

# --- class-level @x from inside a block -------------------------------------
# The block captures `self` as the class object, so the ivar has to resolve
# to the same storage the direct path uses.
class Collect
  @seen = []
  def self.run
    [1, 2, 3].each { |i| @seen << i * 10 }
    @seen
  end
end
p Collect.run

# --- dispatching over a class held in a variable ----------------------------
# The receiver isn't a literal constant here, so this can't be a direct call
# -- it dispatches on the class value at runtime.
class HandlerA
  def self.run(x) = "A:#{x}"
end
class HandlerB
  def self.run(x) = "B:#{x}"
end

[HandlerA, HandlerB].each { |h| puts h.run(1) }

current = HandlerA
puts current.run(2)
current = HandlerB
puts current.run(2)

# --- full parameter shapes on a class method --------------------------------
class Sig
  def self.m(a, b = 2, *rest, k: 9, **kw, &blk)
    [a, b, rest, k, kw, blk ? blk.call : nil]
  end
end
p Sig.m(1)
p Sig.m(1, 3, 4, 5, k: 0, z: 1) { "blk" }

# --- reflection over a class object's ivars ---------------------------------
class Reflect
  @a = 1
end
p Reflect.instance_variable_get(:@a)
p Reflect.instance_variable_get("@a")     # a String name works too
p Reflect.instance_variable_get(:@absent) # nil, not an error
p Reflect.instance_variable_set(:@b, 2)   # answers the VALUE
p Reflect.instance_variables
p Reflect.instance_variable_defined?(:@a)
p Reflect.instance_variable_defined?(:@absent)

begin
  Reflect.instance_variable_get(:a)       # no leading @
rescue NameError => e
  puts "NameError: #{e.message}"
end
