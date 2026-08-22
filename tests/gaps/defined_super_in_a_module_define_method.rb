# `defined?(super)` inside a `define_method` body installed on a MODULE
# answers as if a super target existed. Ruby answers nil: the block's owner is
# the module, and nothing in the receiver's chain past it defines the name.
#
# The same probe is right for a define_method body on a CLASS with a real
# superclass definition, and right for one with no super target at all -- so
# what the module case gets wrong is narrower than the probe itself. The
# `defining_class` a block body carries is the enclosing lexical class, and a
# `Mod.send(:define_method) { ... }` block is written at the TOP LEVEL, which
# is a different class from the module the row lands on.
#
# Pre-existing, and independent of the MRO occurrence pass -- verified by
# base-check. It matters beyond reflection: `defined?(super) ? super : x` is
# the idiom the corpus uses to write a mixin that may or may not have a
# target, and zeo takes the `super` branch, which then raises.

class Base
  def m = "base"
end
class Sub < Base
  define_method(:m) { "sub(" + (defined?(super) ? "has" : "none") + ")" }
end
p Sub.new.m

class Lone
  define_method(:n) { defined?(super) ? "has" : "none" }
end
p Lone.new.n

module Mx
end
class Host
  include Mx
end
Mx.send(:define_method, :q) { defined?(super) ? "has" : "none" }
p Host.new.q
