# `Module#name` answers only for a class or module a CONSTANT PATH reaches.
# Everything else is nameless and answers nil -- while `#to_s`/`#inspect` still
# render it, which is the whole point of the two being different methods.
module Outer
  class Inner; end
  module Mixin; end
end

p Outer.name
p Outer::Inner.name
p Outer::Mixin.name
p Outer::Inner.inspect
p String.name
p Comparable.name

anon_class = Class.new
anon_module = Module.new
p anon_class.name
p anon_module.name
p anon_class.to_s.start_with?("#<Class:0x")
p anon_module.to_s.start_with?("#<Module:0x")
p anon_class.inspect == anon_class.to_s

# A singleton class is nameless too, however well its owner is named.
p String.singleton_class.name
p String.singleton_class.to_s
p Outer::Inner.singleton_class.name
obj = Object.new
p obj.singleton_class.name

# A `Struct.new` is nameless until a constant holds it.
p Struct.new(:a).name
Point = Struct.new(:x, :y)
p Point.name
p Point.new(1, 2).class.name

# Naming happens on the FIRST constant assignment, and a second one does not
# rename -- so the alias reports the original path.
Named = anon_class
p Named.name
Alias = Named
p Alias.name
p Alias.equal?(Named)

# A subclass of an anonymous class is itself anonymous until named.
child = Class.new(anon_class)
p child.name
p child.superclass.equal?(anon_class)
