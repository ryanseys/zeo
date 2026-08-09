# `Readers = Module.new` (rspec-core writes exactly that) and its block form
# build a module the rest of the program mixes in -- so zeo compiles the pair
# to the `module Readers ... end` they are equivalent to, and `include` becomes
# a static ancestry edge instead of a runtime splice no compiled class can see.
Readers = Module.new
p Readers.class
p Readers.name
p Readers.instance_methods(false)

Greet = Module.new do
  def hello = "hello"
  def self.tag = "greet"
  attr_reader :who
  private def secret = "shh"
  alias hi hello
end
p Greet.name
p Greet.instance_methods(false).sort
p Greet.private_instance_methods(false).sort
p Greet.tag

class Host
  include Greet
  include Readers
  def initialize = @who = "world"
end
h = Host.new
p h.hello, h.hi, h.who
p Host.ancestors.first(4)
p h.is_a?(Greet)
p Greet.instance_method(:hello).owner

# The assignment answers with the MODULE; the `module` keyword answers with its
# body's last statement. Only the first is right here.
p(Val = Module.new { def a = 1 })

# A nested one is named for where it is written, and a later `module` reopens
# the same module rather than defining a second one.
module Outer
  Inner = Module.new do
    def deep = "deep"
  end
end
module Outer
  module Inner
    def deeper = "deeper"
  end
end
class C1
  include Outer::Inner
end
p C1.new.deep, C1.new.deeper
p Outer::Inner.name
p Outer.constants.sort

# A body that names a CONSTANT keeps the runtime spelling: a real module joins
# the constant lookup of everything written inside it, and `Module.new`'s block
# opens no scope of its own -- `X = 1` here writes `Object::X`, and `Module.nesting`
# answers with the enclosing chain, not with this module.
Scoped = Module.new do
  X = 1
  def self.nest = Module.nesting
end
p Scoped.name
p defined?(Scoped::X)
p X
p Scoped.nest
