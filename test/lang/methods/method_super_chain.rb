# `Method#super_method` walks UP the ancestor chain: it answers the same
# method as the next ancestor that defines it, re-seated onto that ancestor --
# so calling the result runs the ancestor's body, not the override it was
# reached through. `nil` once the chain runs out.

module Loud
  def greet = "LOUD"
end

class Base
  def greet = "base"
end

class Middle < Base
  include Loud
  def greet = "middle"
end

class Leaf < Middle
end

m = Middle.new.method(:greet)
p m.call
p m.owner

# Each step up the chain: Middle -> Loud (the included module) -> Base.
first = m.super_method
p first.owner, first.call
second = first.super_method
p second.owner, second.call
p second.super_method

# The receiver never changes -- only where the lookup resumes.
p first.receiver.class

# A leaf that overrides nothing starts at the first real definer, so its
# chain is one shorter.
leaf = Leaf.new.method(:greet)
p leaf.call, leaf.owner
p leaf.super_method.owner

# A method no ancestor redefines has no super at all.
class Solo
  def only = 1
end
p Solo.new.method(:only).super_method

# Builtins participate too: Integer#to_s shadows Kernel#to_s.
p 1.method(:to_s).owner
p 1.method(:to_s).super_method.owner

# UnboundMethod has the same walk.
u = Middle.instance_method(:greet)
p u.owner
p u.super_method.owner
p u.super_method.super_method.owner
p u.super_method.super_method.super_method
p u.super_method.bind(Leaf.new).call

# Class methods walk the singleton chain the same way. Their `#owner` is a
# SINGLETON CLASS (`#<Class:Child>`), which zeo has no object for, so this
# asserts the walk itself rather than the owner's spelling.
class Parent
  def self.build = "parent"
end
class Child < Parent
  def self.build = "child"
end
cm = Child.method(:build)
p cm.call, cm.name
p cm.super_method.call
p cm.super_method.super_method
__END__
"middle"
Middle
Loud
"LOUD"
Base
"base"
nil
Middle
"middle"
Middle
Loud
nil
Integer
Kernel
Middle
Loud
Base
nil
"LOUD"
"child"
:build
"parent"
nil
