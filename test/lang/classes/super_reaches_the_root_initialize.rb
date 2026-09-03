# `super` reaching the root `initialize` -- what every chain bottoms out on,
# and what makes the ordinary `include SomeMixin` + `super` idiom work. It
# used to raise "no superclass method 'initialize'": the runtime `super` walk
# consulted only the registry, never the builtin table. Arity 0 is enforced,
# and `initialize` stays private to reflection.

module Greet
  def initialize
    super
    @greeted = true
  end
  def greeted?; @greeted; end
end
class Person
  include Greet
end
p Person.new.greeted?

class Strict
  def initialize(x); super; end
end
begin
  Strict.new(1)
rescue ArgumentError => e
  p e.message
end

class Ok
  def initialize(x); super(); @x = x; end
  attr_reader :x
end
p Ok.new(42).x

p Object.new.respond_to?(:initialize)
p Object.new.respond_to?(:initialize, true)
__END__
true
"wrong number of arguments (given 1, expected 0)"
42
false
true
