# `super` reaching the root `initialize`, which every chain bottoms out on.
# Object#initialize (BasicObject's, really) takes no arguments and does
# nothing -- it exists so the ordinary `include SomeMixin` + `super` idiom
# works, and so a bare `super` at the top of a chain has something to reach
# instead of raising "no superclass method".

module Greet
  def initialize
    super          # reaches Object#initialize
    @greeted = true
  end
  def greeted?; @greeted; end
end

class Person
  include Greet
end
p Person.new.greeted?

# A class whose only initialize is inherited: bare super forwards nothing.
class Base
  def initialize; super; @base = "b"; end
  attr_reader :base
end
class Derived < Base; end
p Derived.new.base

# Arity 0 is enforced: forwarding an argument to the root initialize raises,
# exactly as CRuby does, so a permissive signature can't hide a real bug.
class Strict
  def initialize(x); super; end   # bare super forwards x -> too many args
end
begin
  Strict.new(1)
rescue ArgumentError => e
  p e.message
end

# super() (explicit empty parens) is how a parameterized initialize reaches
# the root without forwarding.
class Ok
  def initialize(x); super(); @x = x; end
  attr_reader :x
end
p Ok.new(42).x

# initialize is private: reflection does not advertise it.
p Object.new.respond_to?(:initialize)
p Object.new.respond_to?(:initialize, true)
