# A REOPEN of a frozen class is a `FrozenError` and its body never runs -- so
# the methods that body would have installed do not exist. zeo's compile-time
# tables already carry them, so the guard retires them at the reopen's own
# position. Gated on the program calling `freeze` at all: without one the shape
# is unreachable and no name loses its static dispatch.
class Fz
  def already = :already
end
Fz.freeze

begin
  class Fz
    def x = :x
    def y = :y
  end
rescue FrozenError => e
  p [e.class, e.message]
end

p Fz.frozen?
p Fz.instance_methods(false).sort
p Fz.method_defined?(:x)
p Fz.new.respond_to?(:x)
begin; Fz.new.x; rescue NoMethodError => e; p e.class; end
# The body's earlier definition is untouched.
p Fz.new.already

# An UNFROZEN class reopens normally.
class Ok
  def a = :a
end
class Ok
  def b = :b
end
p Ok.new.a
p Ok.new.b
p Ok.instance_methods(false).sort

# A module reopen refuses the same way.
module Mz
  def m = :m
end
Mz.freeze
begin
  module Mz
    def n = :n
  end
rescue FrozenError => e
  p [e.class, e.message]
end
p Mz.instance_methods(false)

# A frozen class still answers every ordinary reflection.
p Fz.name
p Fz.superclass.to_s
p Fz.new.class.to_s
