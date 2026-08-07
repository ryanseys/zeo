# One `def` in a module, included into classes that lay their ivars out
# differently. `codegen::share` emits a widely-inherited body ONCE, over a
# `RubyValue` receiver, and an ivar access in that body carries the SLOT INDEX
# -- so a group whose members disagree about where a name sits must not share.
#
# `analyze::mro` lays slots out furthest-ancestor-first, which makes the module's
# own names sort ahead of the includer's: `A` and `B` below would agree if the
# only difference were their own ivars. A SUPERCLASS is what splits them --
# `Base#initialize`'s `@a` takes slot 0 on `A`, pushing the module's `@x` to
# slot 1, while on `B` it is slot 0.
#
# The two must therefore behave identically despite being emitted differently,
# which is the property `ZEO_VERIFY_SHARE=1` reports on.

class Base
  def initialize
    @a = 99
  end
end

module Counter
  def bump
    @x = (@x || 0) + 1
    "#{@x} #{@x.class} #{@x + 1} #{@x * 2} #{@x - 1} #{@x.abs} #{@x.to_s}"
  end

  def peek = @x
end

class WithSuper < Base
  include Counter
end

class WithoutSuper
  include Counter
end

a = WithSuper.new
b = WithoutSuper.new

p a.bump.split.first
p a.bump.split.first
p b.bump.split.first

p [a.peek, b.peek]

# The layouts really are different, and each class reports only its own names.
p a.instance_variables
p b.instance_variables

# A third includer that shares WithoutSuper's layout exactly still works.
class AlsoWithoutSuper
  include Counter
end
c = AlsoWithoutSuper.new
p c.bump.split.first
p [c.peek, c.instance_variables]

# The module's own method is one definition, however many classes carry it.
p Counter.instance_method(:bump).owner
p [WithSuper.new.method(:bump).owner, WithoutSuper.new.method(:bump).owner]
