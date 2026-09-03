# Five behaviours recorded in comments and docs as known divergences, all
# of which zeo already answers exactly as ruby 4.0.6 does.
#
# Each was fixed at some point and its note left behind, so the record said
# zeo was less correct than it is. The notes are deleted; these rows are
# what replaces them, so a regression is caught rather than re-discovered
# and re-filed.
#
#   1. Mixed Integer/Float comparison past 2**53 (docs/ROADMAP.md)
#   2. A class-level `@x` read through an INHERITED class method
#      (dispatch/send_value.rs)
#   3. `UnboundMethod#bind` onto a subclass that overrides the name
#      (runtime_meta/api.rs)
#   4. `Struct.new("Name", ...)` namespacing (tests/e2e/variables.rs)
#   5. A struct constant as a STATIC superclass (builtins/rstruct.rs)

# 1. Exact, not through `as f64`.
p 2**53 == (2**53).to_f
p (2**53 + 1) == (2**53 + 1).to_f
p 9007199254740993 > 9007199254740992.0
p [2**53 + 1, 2.0**53].max
p 1.send(:<, 2**53 + 1)

# 2. The receiver's own storage, not the ancestor's.
class Parent
  @x = "parent"
  class << self
    attr_reader :x
  end

  def self.show = @x
end
class Child < Parent
  @x = "child"
end
p Parent.show
p Child.show
p Child.x

# 3. The OWNER's body, not the target's override.
class Base
  def m = "base"
end
class Derived < Base
  def m = "derived"
end
p Base.instance_method(:m).bind(Derived.new).call

# 4. `Struct::Name`, not a bare top-level `Name`.
Named = Struct.new("Named", :a)
p Struct::Named
p Named.name

# 5. A struct constant IS a static superclass, in both spellings.
Point = Struct.new(:x, :y)
class Sum < Point
  def total = x + y
end
p Sum.new(1, 2).total
p Sum.ancestors.include?(Point)

class Doubler < Struct.new(:z)
  def twice = z * 2
end
p Doubler.new(4).twice
__END__
true
false
true
9007199254740993
true
"parent"
"child"
"child"
"base"
Struct::Named
"Struct::Named"
3
true
8
