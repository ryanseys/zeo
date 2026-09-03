# `super` written in a module that a class `extend`s, called on a SUBCLASS of
# that class. Zeo materializes the module's methods onto every class in the
# chain, so both `Base` and `Sub` carry their own copy -- and the runtime
# resumed a class-method `super` after the DEFINING class's position in the
# receiver's ancestry. An extended module is not in that ancestry at all, so
# the walk restarted at the top and re-entered the copy one level down:
# `Sub.name` answered `Base`, forever.
#
# minitest's Spec::DSL is written exactly this way -- `def name; defined?(@name)
# ? @name : super; end` -- so every spec class reported its superclass's name.
module NameDSL
  def name
    defined?(@name) ? @name : super
  end

  def inspect
    "[#{self}] #{super}"
  end
end

class Base
  extend NameDSL
end

class Sub < Base
end

class Deeper < Sub
end

p Base.name
p Sub.name
p Deeper.name
p Sub.inspect
p Deeper.inspect

# An instance variable set on the subclass still wins over `super`.
class Named < Base
  @name = "custom"
end
p Named.name
p Named.inspect

# A class's own `def self.x` sits BEFORE the extended module in the singleton
# chain, so the module's `super` must skip it and reach the builtin.
module Tagger
  def to_s
    "T(#{super})"
  end
end

class Host
  extend Tagger

  def self.own = "own"
end

class HostSub < Host
end

p Host.to_s
p HostSub.to_s
p HostSub.own

# Two modules extended into one class chain: `super` walks them in order.
module Outer
  def stack = "outer:#{super}"
end

module Inner
  def stack = "inner:#{super}"
end

class Stacked
  extend Inner
  extend Outer

  def self.stack = "base"
end

class StackedSub < Stacked
end

p Stacked.stack
p StackedSub.stack
__END__
"Base"
"Sub"
"Deeper"
"[Sub] Sub"
"[Deeper] Deeper"
"custom"
"[Named] Named"
"T(Host)"
"T(HostSub)"
"own"
"base"
"base"
